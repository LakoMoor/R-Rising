//! R-Rising: the port's executable.
//!
//! It opens a real V Rising install, indexes the original's content archives, uploads the original's
//! meshes and compressed textures to the GPU through wgpu, and draws them. Two modes: a window, and
//! `--offscreen out.png`, which renders one frame without a display so the result can be compared
//! against the original.
//!
//! The loop is a fixed-timestep simulation driving a separate render, the way the original's
//! `ProjectM` bootstrap does; with no gameplay systems ported yet the simulation only advances time.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use rrising_content::{list_archives, GameContent};
use rrising_core::scene::{self, SceneSpec};
use rrising_math::{float3, float4, float4x4};
use rrising_render::{DrawCall, FrameUniforms, Instance, Renderer};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// Where to find the install and what to draw.
struct Options {
    data: PathBuf,
    tpk: PathBuf,
    archives: usize,
    items: usize,
    width: u32,
    height: u32,
    offscreen: Option<PathBuf>,
    export_web: Option<PathBuf>,
    import_web: Option<PathBuf>,
    inspect_scene: Option<PathBuf>,
    type_layouts: Option<PathBuf>,
}

fn parse_args() -> Result<Options> {
    let mut args = std::env::args().skip(1);
    let mut opt = Options {
        // Explicit CLI options override environment variables and portable defaults.
        data: std::env::var_os("VRISING_DATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("../VRising_Data")),
        tpk: std::env::var_os("RRISING_TPK")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("assets/typetrees.tpk")),
        archives: 800,
        items: 24,
        width: 1280,
        height: 720,
        offscreen: None,
        export_web: None,
        import_web: None,
        inspect_scene: None,
        type_layouts: None,
    };
    while let Some(a) = args.next() {
        let mut take = || {
            args.next()
                .ok_or_else(|| anyhow::anyhow!("{a} needs a value"))
        };
        match a.as_str() {
            "--data" => opt.data = PathBuf::from(take()?),
            "--tpk" => opt.tpk = PathBuf::from(take()?),
            "--archives" => opt.archives = take()?.parse().context("--archives takes a number")?,
            "--items" => opt.items = take()?.parse().context("--items takes a number")?,
            "--width" => opt.width = take()?.parse().context("--width takes a number")?,
            "--height" => opt.height = take()?.parse().context("--height takes a number")?,
            "--inspect-scene" => opt.inspect_scene = Some(PathBuf::from(take()?)),
            "--type-layouts" => opt.type_layouts = Some(PathBuf::from(take()?)),
            "--import-web" => opt.import_web = Some(PathBuf::from(take()?)),
            "--export-web" => opt.export_web = Some(PathBuf::from(take()?)),
            "--offscreen" => opt.offscreen = Some(PathBuf::from(take()?)),
            other => return Err(anyhow::anyhow!("unknown argument {other:?}")),
        }
    }
    anyhow::ensure!(
        opt.width > 0 && opt.height > 0,
        "frame dimensions must be positive"
    );
    Ok(opt)
}

/// One drawn object after its GPU buffers exist.
struct Item {
    mesh: usize,
    texture: Option<usize>,
    instances: Vec<Instance>,
}

/// Everything the frame needs, resolved from the original's files and held on the GPU.
struct Loaded {
    meshes: Vec<rrising_render::GpuMesh>,
    textures: Vec<rrising_render::GpuTexture>,
    items: Vec<Item>,
    /// How many items got a real base map rather than the neutral texture.
    textures_bound: usize,
}

impl Loaded {
    /// Borrow the frame's draw list out of the owned buffers.
    fn draws<'a>(&'a self, neutral: &'a wgpu::TextureView) -> Vec<DrawCall<'a>> {
        self.items
            .iter()
            .map(|it| DrawCall {
                mesh: &self.meshes[it.mesh],
                instances: &it.instances,
                texture: match it.texture {
                    Some(t) => &self.textures[t].view,
                    None => neutral,
                },
            })
            .collect()
    }
}

/// Index the content, build the showcase scene and move it onto the GPU.
fn load_scene(renderer: &Renderer, content: &GameContent, spec: &SceneSpec) -> Result<Loaded> {
    let mut meshes = Vec::new();
    let mut textures: Vec<rrising_render::GpuTexture> = Vec::new();
    let mut items = Vec::new();
    let mut textures_bound = 0usize;

    for item in &spec.items {
        let mesh = content
            .read_mesh(&item.mesh)
            .with_context(|| format!("reading mesh {:?}", item.name))?;
        let gpu_mesh = renderer.upload_mesh(&mesh)?;
        let mesh_index = meshes.len();
        meshes.push(gpu_mesh);

        // The material decides which texture this draw binds. A mesh with no material still draws
        // against the neutral 1x1, so its shape is visible and the gap is honest.
        let mut texture_index = None;
        if let Some(mat_ref) = &item.material {
            let Ok(mat) = content.read_material(mat_ref) else {
                log::debug!("no material for {:?}", item.name);
                items.push(Item {
                    mesh: mesh_index,
                    texture: None,
                    instances: vec![Instance::from_matrix(&item.transform)],
                });
                continue;
            };
            let want = rrising_core::base_map_name(&mat);
            let tex_ref = want.and_then(|slot| {
                mat.textures
                    .iter()
                    .find(|(name, _)| name == slot)
                    .and_then(|(_, env)| env.texture.clone())
            });
            if let Some(tex_ref) = tex_ref {
                match content.read_texture(&tex_ref) {
                    Ok(tex) => match renderer.upload_texture(&tex) {
                        Ok(gpu) => {
                            log::info!(
                                "draw {:?} binds slot {want:?} → {:?} {}x{} fmt {}",
                                item.name,
                                tex.name,
                                gpu.width,
                                gpu.height,
                                tex.format
                            );
                            texture_index = Some(textures.len());
                            textures.push(gpu);
                            textures_bound += 1;
                        }
                        Err(e) => log::info!("texture {:?} not uploadable: {e}", tex.name),
                    },
                    Err(e) => log::debug!("reading texture for {:?}: {e}", item.name),
                }
            }
        }
        items.push(Item {
            mesh: mesh_index,
            texture: texture_index,
            instances: vec![Instance::from_matrix(&item.transform)],
        });
    }
    Ok(Loaded {
        meshes,
        textures,
        items,
        textures_bound,
    })
}

fn frame_uniforms(aspect: f32) -> FrameUniforms {
    // A camera looking at the grid the scene builder lays out.
    let eye = float3::new(2.0, 3.0, -8.0);
    let target = float3::new(2.0, 1.0, 4.0);
    // `float4x4.LookAt` is the camera's local-to-world matrix — the original keeps the eye in the
    // fourth column and the forward axis in the third — so the view matrix is its inverse. The z
    // axis is then negated because `PerspectiveFov` expects a point in front of the camera at a
    // negative view-space z (Unity's own `worldToCameraMatrix` applies the same flip engine-side).
    let camera_to_world = float4x4::look_at(eye, target, float3::new(0.0, 1.0, 0.0));
    let view = float4x4::scale(float3::new(1.0, 1.0, -1.0)) * camera_to_world.inverse();
    let proj = float4x4::perspective_fov(1.0472, aspect, 0.1, 200.0);
    let view_proj = rrising_render::clip_fix() * proj * view;
    FrameUniforms {
        view_proj: Instance::from_matrix(&view_proj).model,
        camera_pos: [eye.x, eye.y, eye.z, 0.0],
        // xyz is the light direction, w its strength — the layout the shader declares.
        light_dir: [0.35, 0.85, 0.4, 0.85],
        ambient: [0.32, 0.36, 0.44, 0.0],
    }
}
/// Print the first few items' bounds before anything is drawn, so an empty frame can be told apart
/// from an off-screen one: the mesh's own box, the box the GPU rasterizes after `view_proj`, and how
/// many of its corners land behind the camera.
fn probe_scene(content: &GameContent, spec: &SceneSpec, vp: &[[f32; 4]; 4]) -> Result<()> {
    let view_proj = float4x4::new(
        float4::new(vp[0][0], vp[0][1], vp[0][2], vp[0][3]),
        float4::new(vp[1][0], vp[1][1], vp[1][2], vp[1][3]),
        float4::new(vp[2][0], vp[2][1], vp[2][2], vp[2][3]),
        float4::new(vp[3][0], vp[3][1], vp[3][2], vp[3][3]),
    );
    for item in spec.items.iter().take(3) {
        let mesh = content
            .read_mesh(&item.mesh)
            .with_context(|| format!("probing {:?}", item.name))?;
        let pos = mesh.channels[0]
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("mesh {:?} has no position channel", mesh.name))?;
        let dim = pos.dim.max(1);
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for v in pos.values.chunks(dim) {
            for (i, &x) in v.iter().enumerate().take(3) {
                lo[i] = lo[i].min(x);
                hi[i] = hi[i].max(x);
            }
        }
        let mut behind = 0usize;
        let mut nlo = [f32::MAX; 3];
        let mut nhi = [f32::MIN; 3];
        for c in 0..8 {
            let corner = float3::new(
                if c & 1 == 0 { lo[0] } else { hi[0] },
                if c & 2 == 0 { lo[1] } else { hi[1] },
                if c & 4 == 0 { lo[2] } else { hi[2] },
            );
            let clip =
                view_proj * (item.transform * float4::new(corner.x, corner.y, corner.z, 1.0));
            if clip.w <= 0.0 {
                behind += 1;
                continue;
            }
            let ndc = [clip.x / clip.w, clip.y / clip.w, clip.z / clip.w];
            for i in 0..3 {
                nlo[i] = nlo[i].min(ndc[i]);
                nhi[i] = nhi[i].max(ndc[i]);
            }
        }
        println!(
            "probe {:?}: verts {} inds {} mesh box [{:.2} {:.2} {:.2}]..[{:.2} {:.2} {:.2}] → ndc x [{:.2}..{:.2}] y [{:.2}..{:.2}] z [{:.2}..{:.2}], {} of 8 corners behind the camera",
            item.name,
            mesh.vertex_count,
            mesh.indices.len(),
            lo[0],
            lo[1],
            lo[2],
            hi[0],
            hi[1],
            hi[2],
            nlo[0],
            nhi[0],
            nlo[1],
            nhi[1],
            nlo[2],
            nhi[2],
            behind
        );
    }
    Ok(())
}

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    loaded: Option<Loaded>,
    start: Option<std::time::Instant>,
    frames: u64,
}

impl App {
    fn frame(&mut self) {
        let (Some(r), Some(l)) = (&mut self.renderer, &self.loaded) else {
            return;
        };
        let (w, h) = match &self.window {
            Some(win) => {
                let s = win.inner_size();
                (s.width.max(1), s.height.max(1))
            }
            None => (1280, 720),
        };
        let uniforms = frame_uniforms(w as f32 / h as f32);
        let neutral = r.neutral_view();
        let draws = l.draws(&neutral);
        if let Err(e) = r.render_surface(&uniforms, &draws) {
            log::error!("render: {e}");
        }
        self.frames += 1;
        if self.frames % 120 == 0 {
            println!(
                "frame {} at {:.1} ms/frame — {} items, {} textures bound",
                self.frames,
                self.start
                    .map(|s| s.elapsed().as_secs_f64() * 1000.0 / self.frames as f64)
                    .unwrap_or(0.0),
                l.items.len(),
                l.textures_bound
            );
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = match el.create_window(Window::default_attributes().with_title("R-Rising")) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("no window: {e}");
                el.exit();
                return;
            }
        };
        self.start = Some(std::time::Instant::now());
        let renderer = match pollster::block_on(Renderer::new(window.clone())) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("gpu: {e}");
                el.exit();
                return;
            }
        };
        match run_windowed(&renderer) {
            Ok(loaded) => {
                self.loaded = Some(loaded);
                self.renderer = Some(renderer);
                self.window = Some(window);
            }
            Err(e) => {
                eprintln!("{e:#}");
                el.exit();
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(s) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(s.width.max(1), s.height.max(1));
                }
            }
            WindowEvent::RedrawRequested => {
                self.frame();
                if let Some(w) = &self.window {
                    w.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn open_content(opt: &Options) -> Result<GameContent> {
    let dir = opt.data.join("StreamingAssets/ContentArchives");
    let catalog_path = dir.join("archive_dependencies.bin");
    let all = if catalog_path.is_file() {
        let catalog = rrising_content::catalog::RuntimeContentCatalog::open(&catalog_path)?;
        let paths = catalog.archive_paths(&dir);
        for path in &paths {
            anyhow::ensure!(
                path.is_file(),
                "catalog archive is absent: {}",
                path.display()
            );
        }
        println!(
            "catalog: {} physical archives, {} files, {} object locations, {} dependency sets",
            paths.len(),
            catalog.files.len(),
            catalog.objects.len(),
            catalog.dependency_sets.len()
        );
        paths
    } else {
        log::warn!(
            "no catalog at {}; scanning archives",
            catalog_path.display()
        );
        list_archives(&dir).with_context(|| format!("listing {}", dir.display()))?
    };
    let take = opt.archives.min(all.len());
    println!(
        "opening {} of {} content archives from {}",
        take,
        all.len(),
        dir.display()
    );
    let t = std::time::Instant::now();
    let content = GameContent::open(&opt.data, &all[..take], &opt.tpk)?;
    let counts = content.class_counts();
    println!(
        "indexed {} objects from {} files in {:.1}s",
        counts.values().sum::<usize>(),
        content.file_count(),
        t.elapsed().as_secs_f64()
    );
    for (k, v) in counts.iter().take(8) {
        println!("  {k}: {v}");
    }
    Ok(content)
}

fn run_windowed(renderer: &Renderer) -> Result<Loaded> {
    let opt = parse_args()?;
    let content = open_content(&opt)?;
    let spec = scene::showcase(&content, opt.items);
    println!("scene: {} items", spec.len());
    load_scene(renderer, &content, &spec)
}

fn run_offscreen(opt: &Options) -> Result<()> {
    let renderer = pollster::block_on(Renderer::new_offscreen(opt.width, opt.height))?;
    if let Some(path) = &opt.import_web {
        let snapshot: rrising_render::snapshot::Snapshot =
            serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(path)?))?;
        let loaded = snapshot.upload(&renderer)?;
        let mut renderer = renderer;
        let (w, h, rgba) = loaded.render_to_image(&mut renderer)?;
        let output = opt
            .offscreen
            .as_ref()
            .context("--import-web requires --offscreen")?;
        write_png(output, w, h, &rgba)?;
        println!(
            "rendered {} transferred objects to {}",
            loaded.item_count(),
            output.display()
        );
        return Ok(());
    }
    let content = open_content(opt)?;
    let spec = scene::showcase(&content, opt.items);
    println!("scene: {} items", spec.len());
    let loaded = load_scene(&renderer, &content, &spec)?;
    let mut renderer = renderer;
    let uniforms = frame_uniforms(opt.width as f32 / opt.height as f32);
    probe_scene(&content, &spec, &uniforms.view_proj)?;
    let neutral = renderer.neutral_view();
    let draws = loaded.draws(&neutral);
    let (w, h, rgba) = renderer.render_to_image(&uniforms, &draws)?;
    let path = opt
        .offscreen
        .clone()
        .unwrap_or_else(|| PathBuf::from("frame.png"));
    write_png(&path, w, h, &rgba)?;
    let clear = (rgba[0], rgba[1], rgba[2]);
    let drawn = rgba
        .chunks(4)
        .filter(|p| (p[0], p[1], p[2]) != clear)
        .count();
    println!(
        "wrote {} ({}x{}, {} items, {} textures bound, {drawn} pixels covered by geometry, clear {clear:?})",
        path.display(),
        w,
        h,
        loaded.items.len(),
        loaded.textures_bound
    );
    Ok(())
}

/// Encode RGBA8 as a PNG.
fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<()> {
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec())
        .ok_or_else(|| anyhow::anyhow!("bad readback size"))?;
    img.save(path)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// Export only to the owner's local disk. Scene transfers contain original art and are not published.
fn export_web(opt: &Options, path: &Path) -> Result<()> {
    use rrising_render::snapshot::{Snapshot, SnapshotItem};
    let content = open_content(opt)?;
    let scene = scene::showcase(&content, opt.items);
    let mut items = Vec::new();
    for item in scene.items {
        let mesh = content.read_mesh(&item.mesh)?;
        let (vertices, indices) = rrising_render::mesh::build_vertices(&mesh)?;
        let texture = if let Some(material) = item.material {
            let material = content.read_material(&material)?;
            rrising_core::base_map_name(&material)
                .and_then(|slot| material.textures.iter().find(|(name, _)| name == slot))
                .and_then(|(_, env)| env.texture.as_ref())
                .map(|object| -> Result<_> {
                    let texture = content.read_texture(object)?;
                    rrising_render::unity_texture::export(&texture)
                })
                .transpose()?
        } else {
            None
        };
        items.push(SnapshotItem {
            name: item.name,
            vertices,
            indices,
            transform: Instance::from_matrix(&item.transform),
            texture,
        });
    }
    let snapshot = Snapshot {
        version: 1,
        description: "Original art in a showcase grid; no gameplay or original world placement"
            .into(),
        uniforms: frame_uniforms(opt.width as f32 / opt.height as f32),
        items,
    };
    let file =
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer(&mut writer, &snapshot)?;
    use std::io::Write;
    writer.flush()?;
    println!(
        "exported {} objects to {} (local original content; excluded from Git)",
        snapshot.items.len(),
        path.display()
    );
    Ok(())
}

/// Read original component bytes through recovered storage layout, without simulation.
fn inspect_scene(opt: &Options, path: &Path) -> Result<()> {
    use rrising_content::type_layouts::{TypeLayouts, LOCAL_TO_WORLD_HASH, TRANSLATION_HASH};
    let layout_path = opt.type_layouts.as_ref().context(
        "--inspect-scene requires --type-layouts generated by tools/probe_type_registry.py",
    )?;
    let layouts = TypeLayouts::from_json(&std::fs::read(layout_path)?)?;
    let scene = rrising_content::entity_scenes::SceneFile::open(path)?;
    let resolved = layouts.resolve_scene(&scene)?;
    let mut samples = Vec::new();
    for (index, chunk) in resolved.chunks.iter().enumerate() {
        for entity in 0..chunk.header.count as usize {
            if samples.len() >= 8 {
                break;
            }
            let translation = resolved.component_f32s::<3>(index, entity, TRANSLATION_HASH)?;
            let local_to_world =
                resolved.component_f32s::<16>(index, entity, LOCAL_TO_WORLD_HASH)?;
            if translation.is_some() || local_to_world.is_some() {
                samples.push(serde_json::json!({ "chunk": index, "entity_in_chunk": entity,
                    "translation": translation, "local_to_world": local_to_world.map(|m|m.to_vec()) }));
            }
        }
        if samples.len() >= 8 {
            break;
        }
    }
    let metadata = rrising_content::scene_runtime::load_chunk_metadata(&scene, 1)?;
    let result = serde_json::json!({
        "mode": "read_only_original_scene", "scene": path, "simulation": false,
        "total_archetypes": resolved.summary.total_archetypes,
        "resolved_archetypes": resolved.summary.resolved_archetypes,
        "unresolved_archetypes": resolved.summary.unresolved_archetypes.len(),
        "resolved_chunks": resolved.summary.resolved_chunks,
        "total_chunks": resolved.summary.total_chunks,
        "resolved_entities": resolved.summary.resolved_entities,
        "buffer_headers_checked": resolved.summary.buffer_headers,
        "external_buffer_patches_checked": resolved.summary.external_buffer_patches,
        "runtime_metadata_chunks": metadata.iter().map(|m|m.count()).sum::<usize>(),
        "component_samples": samples,
        "limitation": "Recovered storage with chunk-boundary checks; not a full TypeInfo oracle or game world"
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn main() -> Result<()> {
    env_logger::init();
    let opt = parse_args()?;
    if let Some(path) = &opt.inspect_scene {
        return inspect_scene(&opt, path);
    }
    if let Some(path) = &opt.export_web {
        return export_web(&opt, path);
    }
    if opt.offscreen.is_some() {
        return run_offscreen(&opt);
    }
    let el = EventLoop::new()?;
    el.set_control_flow(ControlFlow::Poll);
    let mut app = App::default();
    el.run_app(&mut app)?;
    Ok(())
}
