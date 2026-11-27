//! Small alpha-blended image overlay. No Unity canvas or input semantics are implied.
use wgpu::util::DeviceExt;

/// The currently supported diagnostic action-bar layout, in assumed 1080-height canvas units.
/// This is not the original CanvasScaler or an inventory state.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ActionBarLayout {
    pub slot_size: [f32; 2],
    pub frame_extra: [f32; 2],
    pub anchor_position: [f32; 2],
    pub spacing: f32,
    pub count: usize,
}

impl ActionBarLayout {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!((1..=64).contains(&self.count), "invalid action bar count");
        anyhow::ensure!(
            self.slot_size
                .iter()
                .chain(&self.frame_extra)
                .chain(&self.anchor_position)
                .chain([&self.spacing])
                .all(|v| v.is_finite()),
            "nonfinite action bar layout"
        );
        anyhow::ensure!(
            self.slot_size.iter().all(|v| *v > 0.0)
                && self.frame_extra.iter().all(|v| *v >= 0.0)
                && self.spacing >= 0.0,
            "invalid action bar dimensions"
        );
        Ok(())
    }

    pub fn rects(&self, width: u32, height: u32) -> Vec<[f32; 4]> {
        let scale = height as f32 / 1080.0;
        let total = self.count as f32 * self.slot_size[0]
            + self.count.saturating_sub(1) as f32 * self.spacing;
        let right = width as f32 * 0.5 + self.anchor_position[0] * scale;
        let center_y = height as f32 - self.anchor_position[1] * scale;
        (0..self.count)
            .map(|i| {
                [
                    right - total * scale + i as f32 * (self.slot_size[0] + self.spacing) * scale
                        - self.frame_extra[0] * scale * 0.5,
                    center_y - (self.slot_size[1] + self.frame_extra[1]) * scale * 0.5,
                    (self.slot_size[0] + self.frame_extra[0]) * scale,
                    (self.slot_size[1] + self.frame_extra[1]) * scale,
                ]
            })
            .collect()
    }
}

/// Pixel rectangles (x, y, width, height) in top-left viewport coordinates.
pub struct OverlayDraw<'a> {
    pub texture: &'a wgpu::TextureView,
    pub rects: &'a [[f32; 4]],
    /// Texture coordinates (left, top, right, bottom) after Unity texture upload.
    pub uv: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    position: [f32; 2],
    uv: [f32; 2],
}

pub(crate) struct OverlayPipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl OverlayPipeline {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("overlay.wgsl"));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("overlay textures"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("overlay"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("alpha overlay"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs,
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("overlay sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            layout,
            sampler,
        }
    }

    pub fn encode(
        &self,
        device: &wgpu::Device,
        enc: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        size: [u32; 2],
        draws: &[OverlayDraw<'_>],
    ) {
        if draws.is_empty() {
            return;
        }
        let mut prepared = Vec::new();
        for draw in draws {
            let mut vertices = Vec::new();
            for &[x, y, w, h] in draw.rects {
                if ![x, y, w, h].iter().all(|v| v.is_finite()) || w <= 0.0 || h <= 0.0 {
                    continue;
                }
                let (l, r) = (
                    x * 2.0 / size[0] as f32 - 1.0,
                    (x + w) * 2.0 / size[0] as f32 - 1.0,
                );
                let (t, b) = (
                    1.0 - y * 2.0 / size[1] as f32,
                    1.0 - (y + h) * 2.0 / size[1] as f32,
                );
                let [u0, v0, u1, v1] = draw.uv;
                let (a, bv, c, d) = (
                    Vertex {
                        position: [l, t],
                        uv: [u0, v0],
                    },
                    Vertex {
                        position: [r, t],
                        uv: [u1, v0],
                    },
                    Vertex {
                        position: [l, b],
                        uv: [u0, v1],
                    },
                    Vertex {
                        position: [r, b],
                        uv: [u1, v1],
                    },
                );
                vertices.extend_from_slice(&[a, bv, c, c, bv, d]);
            }
            if vertices.is_empty() {
                continue;
            }
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("overlay vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("overlay image"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(draw.texture),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            prepared.push((buffer, bind, vertices.len() as u32));
        }
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        for (buffer, bind, count) in &prepared {
            pass.set_bind_group(0, bind, &[]);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..*count, 0..1);
        }
    }
}
