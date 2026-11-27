//! The port's renderer: a wgpu device, the ported pipeline, and an offscreen path that reads frames
//! back so they can be compared against the original's screenshots.
//!
//! Depth convention: the port builds its matrices with the ported `Unity.Mathematics` functions, so
//! they produce Unity's clip space. wgpu's depth range is [0, 1], so one fixed conversion matrix
//! remaps z. That is the only coordinate-space change in the pipeline and it lives here, not in the
//! math crate, so the ported matrices stay byte-identical to the original's.

use anyhow::{anyhow, Context, Result};
use rrising_math::float4x4;
use wgpu::util::DeviceExt;

use crate::mesh::{self, GpuMesh};
#[cfg(feature = "unity")]
use crate::texture::GpuTexture;
#[cfg(feature = "unity")]
use crate::unity_texture;

/// One instance: a model matrix, column-major, 64 bytes.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, serde::Serialize, serde::Deserialize)]
pub struct Instance {
    pub model: [[f32; 4]; 4],
}

impl Instance {
    #[inline]
    pub fn from_matrix(m: &float4x4) -> Self {
        Self {
            model: [
                [m.c0.x, m.c0.y, m.c0.z, m.c0.w],
                [m.c1.x, m.c1.y, m.c1.z, m.c1.w],
                [m.c2.x, m.c2.y, m.c2.z, m.c2.w],
                [m.c3.x, m.c3.y, m.c3.z, m.c3.w],
            ],
        }
    }
}

/// The per-frame uniform block. 128 bytes, matching the WGSL `Frame` struct.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, serde::Serialize, serde::Deserialize)]
pub struct FrameUniforms {
    pub view_proj: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    pub light_dir: [f32; 4],
    pub ambient: [f32; 4],
}

/// What to draw this frame.
///
/// The texture is a view rather than a `GpuTexture` so a caller can bind either its own texture or
/// the renderer's neutral one without either being boxed.
pub struct DrawCall<'a> {
    pub mesh: &'a GpuMesh,
    pub instances: &'a [Instance],
    pub texture: &'a wgpu::TextureView,
    /// Alpha-test the base map's alpha at 0.5 (`fs_cutout`) instead of ignoring it.
    pub cutout: bool,
}

/// Unity clip space (z in [-1, 1]) to wgpu's (z in [0, 1]), as a column-major matrix.
pub fn clip_fix() -> float4x4 {
    float4x4::new(
        float4x4_cols(1.0, 0.0, 0.0, 0.0),
        float4x4_cols(0.0, 1.0, 0.0, 0.0),
        float4x4_cols(0.0, 0.0, 0.5, 0.0),
        float4x4_cols(0.0, 0.0, 0.5, 1.0),
    )
}

fn float4x4_cols(x: f32, y: f32, z: f32, w: f32) -> rrising_math::float4 {
    rrising_math::float4::new(x, y, z, w)
}

/// The GPU side of the renderer, with or without a window.
pub struct Renderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: Option<wgpu::Surface<'static>>,
    config: Option<wgpu::SurfaceConfiguration>,
    format: wgpu::TextureFormat,
    pipeline: wgpu::RenderPipeline,
    pipeline_cutout: wgpu::RenderPipeline,
    overlay: crate::overlay::OverlayPipeline,
    bind_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    neutral: wgpu::TextureView,
    uniforms: wgpu::Buffer,
    depth: Option<(wgpu::Texture, wgpu::TextureView)>,
    width: u32,
    height: u32,
}

async fn create_device(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'static>>,
    offscreen: bool,
) -> Result<(wgpu::Adapter, wgpu::Device, wgpu::Queue)> {
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: surface,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        })
        .await
        .map_err(|e| anyhow!("no GPU adapter: {e}"))?;
    // The art is BC-compressed, so BC support is taken from the adapter rather than assumed; a
    // readback also needs the adapter-specific format set on the colour target.
    let mut required_features = adapter.features() & wgpu::Features::TEXTURE_COMPRESSION_BC;
    if offscreen {
        required_features |= wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
    }
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("rrising"),
            required_features,
            ..Default::default()
        })
        .await
        .map_err(|e| anyhow!("requesting a device: {e}"))?;
    Ok((adapter, device, queue))
}

impl Renderer {
    /// A renderer bound to a window.
    pub async fn new(window: std::sync::Arc<winit::window::Window>) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window)
            .map_err(|e| anyhow!("creating a surface: {e}"))?;
        let (adapter, device, queue) = create_device(&instance, Some(&surface), false).await?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
            color_space: Default::default(),
        };
        surface.configure(&device, &config);
        Ok(Self::build(
            device,
            queue,
            &adapter,
            Some((surface, config)),
            format,
            size.width.max(1),
            size.height.max(1),
        ))
    }

    /// Browser surface, with asynchronous WebGPU initialization.
    #[cfg(target_arch = "wasm32")]
    pub async fn from_canvas(canvas: web_sys::HtmlCanvasElement) -> Result<Self> {
        let (width, height) = (canvas.width().max(1), canvas.height().max(1));
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(wgpu::SurfaceTarget::Canvas(canvas))?;
        let (adapter, device, queue) = create_device(&instance, Some(&surface), false).await?;
        let caps = surface.get_capabilities(&adapter);
        let surface_format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let format = surface_format.add_srgb_suffix();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: if format != surface_format {
                vec![format]
            } else {
                vec![]
            },
            desired_maximum_frame_latency: 2,
            color_space: Default::default(),
        };
        surface.configure(&device, &config);
        Ok(Self::build(
            device,
            queue,
            &adapter,
            Some((surface, config)),
            format,
            width,
            height,
        ))
    }

    /// A renderer with no window, for offscreen frames and tests.
    pub async fn new_offscreen(width: u32, height: u32) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let (adapter, device, queue) = create_device(&instance, None, true).await?;
        // Offscreen frames land in an RGBA8 target, so a golden PNG is byte-comparable.
        Ok(Self::build(
            device,
            queue,
            &adapter,
            None,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            width,
            height,
        ))
    }

    fn build(
        device: wgpu::Device,
        queue: wgpu::Queue,
        adapter: &wgpu::Adapter,
        window: Option<(wgpu::Surface<'static>, wgpu::SurfaceConfiguration)>,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let _ = adapter;
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rrising"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let make_pipeline = |label: &str, fragment: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &mesh::vertex_layout(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fragment),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    // The original's winding is not yet recovered from the decompiled build, so the port
                    // draws both faces rather than guess and cull half of every model. Recorded in
                    // docs/PORT.md as an open item.
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: if debug_no_depth() {
                    None
                } else {
                    Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(wgpu::CompareFunction::Less),
                        stencil: wgpu::StencilState::default(),
                        bias: wgpu::DepthBiasState::default(),
                    })
                },
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = make_pipeline("lit", "fs_main");
        let pipeline_cutout = make_pipeline("lit cutout", "fs_cutout");
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("albedo"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        // A 1x1 white texture so a draw with no material still shows the mesh's shape.
        let neutral_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("neutral"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &neutral_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &[255u8, 255, 255, 255],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("frame uniforms"),
            size: std::mem::size_of::<FrameUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (surface, config) = match window {
            Some((s, c)) => (Some(s), Some(c)),
            None => (None, None),
        };
        let overlay = crate::overlay::OverlayPipeline::new(&device, format);
        let mut me = Self {
            device,
            queue,
            surface,
            config,
            format,
            pipeline,
            pipeline_cutout,
            overlay,
            bind_layout,
            sampler,
            neutral: neutral_tex.create_view(&Default::default()),
            uniforms,
            depth: None,
            width,
            height,
        };
        me.ensure_depth(width, height);
        me
    }

    /// The window's surface format, so the app can match its clear colour.
    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// The device, so a test can build the buffers it wants to draw.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The queue, so a test can fill those buffers.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn resize(&mut self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        self.width = w;
        self.height = h;
        self.ensure_depth(w, h);
        if let (Some(cfg), Some(surf)) = (&mut self.config, &self.surface) {
            cfg.width = w;
            cfg.height = h;
            surf.configure(&self.device, cfg);
        }
    }

    fn ensure_depth(&mut self, w: u32, h: u32) {
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        self.depth = Some((tex, view));
    }

    /// The 1x1 white view a draw uses when its material has no base map.
    /// Returned by value so a caller can hold it while rendering through `&mut self`.
    pub fn neutral_view(&self) -> wgpu::TextureView {
        self.neutral.clone()
    }

    /// Upload a mesh.
    #[cfg(feature = "unity")]
    pub fn upload_mesh(&self, m: &unity_formats::mesh::UnityMeshData) -> Result<GpuMesh> {
        mesh::upload(&self.device, m)
    }

    /// Upload a texture.
    #[cfg(feature = "unity")]
    pub fn upload_texture(&self, t: &unity_formats::texture::UnityTexture) -> Result<GpuTexture> {
        // The adapter is asked again through the device's features; BC support is a device feature.
        unity_texture::upload(&self.device, &self.queue, t, self.device.features())
    }

    fn bind_group(&self, view: &wgpu::TextureView) -> wgpu::BindGroup {
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame bind"),
            layout: &self.bind_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    /// Draw into the configured surface.
    pub fn render_surface(
        &mut self,
        uniforms: &FrameUniforms,
        draws: &[DrawCall<'_>],
    ) -> Result<()> {
        self.render_surface_with_overlay(uniforms, draws, &[])
    }

    pub fn render_surface_with_overlay(
        &mut self,
        uniforms: &FrameUniforms,
        draws: &[DrawCall<'_>],
        overlay: &[crate::overlay::OverlayDraw<'_>],
    ) -> Result<()> {
        use wgpu::CurrentSurfaceTexture as T;
        let surface = self
            .surface
            .as_ref()
            .ok_or_else(|| anyhow!("render_surface needs a window"))?;
        let frame = match surface.get_current_texture() {
            T::Success(f) | T::Suboptimal(f) => f,
            T::Outdated | T::Lost => {
                if let Some(cfg) = &self.config {
                    surface.configure(&self.device, cfg);
                }
                return Ok(());
            }
            _ => return Ok(()),
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.format),
            ..Default::default()
        });
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(uniforms));
        let depth_view = self
            .depth
            .as_ref()
            .map(|(_, v)| v.clone())
            .ok_or_else(|| anyhow!("no depth buffer"))?;
        // `encode` submits what it drew; the frame is presented after it.
        self.encode(&view, &depth_view, self.format, draws, overlay);
        self.queue.present(frame);
        Ok(())
    }

    /// Render offscreen and read the frame back as RGBA8, top row first.
    pub fn render_to_image(
        &mut self,
        uniforms: &FrameUniforms,
        draws: &[DrawCall<'_>],
    ) -> Result<(u32, u32, Vec<u8>)> {
        self.render_to_image_with_overlay(uniforms, draws, &[])
    }

    pub fn render_to_image_with_overlay(
        &mut self,
        uniforms: &FrameUniforms,
        draws: &[DrawCall<'_>],
        overlay: &[crate::overlay::OverlayDraw<'_>],
    ) -> Result<(u32, u32, Vec<u8>)> {
        let w = self.width;
        let h = self.height;
        let color = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen color"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = color.create_view(&Default::default());
        let depth_view = self
            .depth
            .as_ref()
            .map(|(_, v)| v.clone())
            .ok_or_else(|| anyhow!("no depth buffer"))?;
        self.queue
            .write_buffer(&self.uniforms, 0, bytemuck::bytes_of(uniforms));
        self.encode(
            &view,
            &depth_view,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            draws,
            overlay,
        );

        // Read back with the 256-byte row alignment wgpu requires.
        let unaligned = w * 4;
        let bytes_per_row = (unaligned + 255) & !255;
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(bytes_per_row) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &color,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([enc.finish()]);

        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        rx.recv()
            .context("no readback result")?
            .context("mapping the readback buffer")?;
        let padded: Vec<u8> = slice.get_mapped_range()?.to_vec();
        staging.unmap();

        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as usize {
            let start = y * bytes_per_row as usize;
            out.extend_from_slice(&padded[start..start + unaligned as usize]);
        }
        Ok((w, h, out))
    }

    fn encode(
        &self,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        format: wgpu::TextureFormat,
        draws: &[DrawCall<'_>],
        overlay: &[crate::overlay::OverlayDraw<'_>],
    ) {
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rrising main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: color,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.02,
                            g: 0.02,
                            b: 0.03,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: if debug_no_depth() {
                    None
                } else {
                    Some(wgpu::RenderPassDepthStencilAttachment {
                        view: depth,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(1.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    })
                },
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group(&self.neutral), &[]);
            let _ = format;
            let mut cutout_bound = false;
            for d in draws {
                if d.cutout != cutout_bound {
                    pass.set_pipeline(if d.cutout {
                        &self.pipeline_cutout
                    } else {
                        &self.pipeline
                    });
                    cutout_bound = d.cutout;
                }
                pass.set_bind_group(0, &self.bind_group(d.texture), &[]);
                let inst = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some(&format!("inst {}", d.mesh.name)),
                        contents: bytemuck::cast_slice(d.instances),
                        usage: wgpu::BufferUsages::VERTEX,
                    });
                pass.set_vertex_buffer(0, d.mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(d.mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.set_vertex_buffer(1, inst.slice(..));
                pass.draw_indexed(0..d.mesh.index_count as u32, 0, 0..d.instances.len() as u32);
            }
        }
        self.overlay.encode(
            &self.device,
            &mut enc,
            color,
            [self.width, self.height],
            overlay,
        );
        self.queue.submit([enc.finish()]);
    }
}

/// The depth format the pipeline and every target agree on.
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// A debug switch for the port's own tests: `RRISING_DEBUG_NO_DEPTH=1` drops the depth attachment
/// and the pipeline's depth state, so an empty frame can be attributed to depth rather than to the
/// vertex path. Normal runs never set it.
fn debug_no_depth() -> bool {
    std::env::var_os("RRISING_DEBUG_NO_DEPTH").is_some()
}
