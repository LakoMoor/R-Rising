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

mod scene_view;

use anyhow::{Context, Result};
use rrising_content::persistence::{PersistentSave, SavedSnapshot, CONTAINER_VERSION};
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
    inspect_save: Option<PathBuf>,
    restore_projection: bool,
    scene: Option<PathBuf>,
    type_layouts: Option<PathBuf>,
    /// `--scene-sim N`: run fixed steering/transform frames and draw the World's LocalToWorld.
    scene_sim: Option<usize>,
    /// `--compare out.json`: stored-vs-simulated image comparison for `--scene`.
    compare: Option<PathBuf>,
    bench: bool,
    frames: usize,
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
        inspect_save: None,
        restore_projection: false,
        scene: None,
        type_layouts: None,
        scene_sim: None,
        compare: None,
        bench: false,
        frames: 60,
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
            "--inspect-save" => opt.inspect_save = Some(PathBuf::from(take()?)),
            "--restore-projection" => opt.restore_projection = true,
            "--scene" => opt.scene = Some(PathBuf::from(take()?)),
            "--scene-sim" => {
                opt.scene_sim = Some(take()?.parse().context("--scene-sim takes a frame count")?)
            }
            "--compare" => opt.compare = Some(PathBuf::from(take()?)),
            "--type-layouts" => opt.type_layouts = Some(PathBuf::from(take()?)),
            "--import-web" => opt.import_web = Some(PathBuf::from(take()?)),
            "--export-web" => opt.export_web = Some(PathBuf::from(take()?)),
            "--bench" => opt.bench = true,
            "--frames" => opt.frames = take()?.parse().context("--frames takes a number")?,
            "--offscreen" => opt.offscreen = Some(PathBuf::from(take()?)),
            other => return Err(anyhow::anyhow!("unknown argument {other:?}")),
        }
    }
    anyhow::ensure!(
        opt.width > 0 && opt.height > 0,
        "frame dimensions must be positive"
    );
    anyhow::ensure!(
        !opt.restore_projection || (opt.inspect_save.is_some() && opt.type_layouts.is_some()),
        "--restore-projection requires --inspect-save, --type-layouts and server data via --data"
    );
    anyhow::ensure!(
        opt.scene_sim.is_none() || (opt.scene.is_some() && opt.scene_sim != Some(0)),
        "--scene-sim needs --scene and a positive frame count"
    );
    Ok(opt)
}

/// One drawn object after its GPU buffers exist.
struct Item {
    mesh: usize,
    texture: Option<usize>,
    cutout: bool,
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
    /// Re-point the instance lists at (possibly simulated) matrices; items follow the group
    /// order, with the marker draw last.
    fn sync(&mut self, data: &scene_view::SceneData) {
        for (it, g) in self.items.iter_mut().zip(&data.groups) {
            it.instances.clone_from(&g.instances);
        }
        if !data.markers.is_empty() {
            if let Some(it) = self.items.get_mut(data.groups.len()) {
                it.instances.clone_from(&data.markers);
            }
        }
    }

    /// Borrow the frame's draw list out of the owned buffers.
    fn draws<'a>(&'a self, neutral: &'a wgpu::TextureView) -> Vec<DrawCall<'a>> {
        self.items
            .iter()
            .map(|it| DrawCall {
                mesh: &self.meshes[it.mesh],
                instances: &it.instances,
                cutout: it.cutout,
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
                    cutout: false,
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
            cutout: false,
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

/// Move an original scene's resolved placements onto the GPU: one instanced draw per
/// (mesh, base map) pair, plus one instanced marker-box draw for everything unresolved.
fn load_scene_view(
    renderer: &Renderer,
    content: &GameContent,
    data: &mut scene_view::SceneData,
) -> Result<Loaded> {
    let mut meshes = Vec::new();
    let mut textures: Vec<rrising_render::GpuTexture> = Vec::new();
    let mut items = Vec::new();
    let mut mesh_slot: std::collections::HashMap<usize, usize> = Default::default();
    let mut tex_slot: std::collections::HashMap<unity_formats::player::ObjRef, Option<usize>> =
        Default::default();
    let mut textures_bound = 0;
    let mut grey_slot: Option<usize> = None;
    let mut array_textures: std::collections::HashMap<unity_formats::player::ObjRef, bool> =
        Default::default();
    let mut array_instances = 0;
    let (mut failed_instances, mut failed_textures) = (0, 0);
    for g in &data.groups {
        let mesh = match mesh_slot.get(&g.mesh_id) {
            Some(&m) => m,
            None => {
                meshes.push(rrising_render::mesh::upload_vertices(
                    renderer.device(),
                    &g.name,
                    &g.verts,
                    &g.inds,
                    1,
                    false,
                )?);
                mesh_slot.insert(g.mesh_id, meshes.len() - 1);
                meshes.len() - 1
            }
        };
        let real = match &g.texture {
            None => None,
            Some(r) => *tex_slot.entry(r.clone()).or_insert_with(|| {
                let uploaded = scene_view::read_base_map(content, r).and_then(|(t, layer0)| {
                    array_textures.insert(r.clone(), layer0);
                    renderer.upload_texture(&t)
                });
                match uploaded {
                    Ok(gpu) => {
                        textures.push(gpu);
                        Some(textures.len() - 1)
                    }
                    Err(e) => {
                        log::warn!(
                            "base map {r:?} ({:?}) not uploaded: {e:#}",
                            content.player().class_name(r)
                        );
                        failed_textures += 1;
                        None
                    }
                }
            }),
        };
        if g.texture.is_some() && real.is_none() {
            failed_instances += g.instances.len();
        }
        if real.is_some()
            && g.texture
                .as_ref()
                .is_some_and(|r| array_textures.get(r) == Some(&true))
        {
            array_instances += g.instances.len();
        }
        // No base map (or one that cannot be held): a flat grey, never the white neutral texture.
        if real.is_none() && grey_slot.is_none() {
            let grey =
                rrising_render::texture::TextureData::flat("fallback grey", [128, 128, 128, 255]);
            if let Ok(t) = grey.upload(
                renderer.device(),
                renderer.queue(),
                renderer.device().features(),
            ) {
                textures.push(t);
                grey_slot = Some(textures.len() - 1);
            }
        }
        let texture = real.or(grey_slot);
        textures_bound += usize::from(real.is_some());
        items.push(Item {
            mesh,
            texture,
            cutout: g.cutout,
            instances: g.instances.clone(),
        });
    }
    data.stats.texture_fallback_instances = failed_instances;
    data.stats.texture_fallback_textures = failed_textures;
    data.stats.array_layer0_instances = array_instances;
    if !data.markers.is_empty() {
        let (v, i) = scene_view::marker_box();
        meshes.push(rrising_render::mesh::upload_vertices(
            renderer.device(),
            "marker box",
            &v,
            &i,
            1,
            false,
        )?);
        items.push(Item {
            mesh: meshes.len() - 1,
            texture: None,
            cutout: false,
            instances: data.markers.clone(),
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
    let proj = float4x4::perspective_fov(std::f32::consts::FRAC_PI_3, aspect, 0.1, 200.0);
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
    /// Fly camera when `--scene` is active: WASD move, Q/E down/up, arrows look, Shift fast.
    fly: Option<scene_view::FlyCam>,
    keys: std::collections::HashSet<winit::keyboard::KeyCode>,
    last_tick: Option<std::time::Instant>,
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
        let uniforms = match &mut self.fly {
            Some(cam) => {
                use winit::keyboard::KeyCode as K;
                let now = std::time::Instant::now();
                let dt = self
                    .last_tick
                    .map(|t| (now - t).as_secs_f32())
                    .unwrap_or(0.0)
                    .min(0.1);
                self.last_tick = Some(now);
                let k = |c: K| f32::from(u8::from(self.keys.contains(&c)));
                let boost = if self.keys.contains(&K::ShiftLeft) {
                    4.0
                } else {
                    1.0
                };
                cam.fly(
                    (k(K::KeyW) - k(K::KeyS)) * boost,
                    (k(K::KeyD) - k(K::KeyA)) * boost,
                    (k(K::KeyE) - k(K::KeyQ)) * boost,
                    dt,
                );
                cam.yaw += (k(K::ArrowRight) - k(K::ArrowLeft)) * dt * 1.2;
                cam.pitch =
                    (cam.pitch + (k(K::ArrowUp) - k(K::ArrowDown)) * dt * 1.2).clamp(-1.5, 1.5);
                cam.uniforms(w as f32 / h as f32)
            }
            None => frame_uniforms(w as f32 / h as f32),
        };
        let neutral = r.neutral_view();
        let draws = l.draws(&neutral);
        if let Err(e) = r.render_surface(&uniforms, &draws) {
            log::error!("render: {e}");
        }
        self.frames += 1;
        if self.frames.is_multiple_of(120) {
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
            Ok((loaded, fly)) => {
                self.fly = fly;
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
            WindowEvent::KeyboardInput { event, .. } => {
                if let winit::keyboard::PhysicalKey::Code(code) = event.physical_key {
                    if event.state.is_pressed() {
                        self.keys.insert(code);
                    } else {
                        self.keys.remove(&code);
                    }
                }
            }
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

fn run_windowed(renderer: &Renderer) -> Result<(Loaded, Option<scene_view::FlyCam>)> {
    let opt = parse_args()?;
    let content = open_content(&opt)?;
    if let Some(scene_path) = &opt.scene {
        let layouts = scene_view::layouts_path(opt.type_layouts.as_ref())?;
        let mut data = scene_view::collect(&content, &opt.data, &layouts, scene_path)?;
        apply_sim(&opt, &layouts, scene_path, &mut data)?;
        let loaded = load_scene_view(renderer, &content, &mut data)?;
        println!("{}", data.stats.report());
        return Ok((loaded, Some(scene_view::FlyCam::framing(data.bounds))));
    }
    let spec = scene::showcase(&content, opt.items);
    println!("scene: {} items", spec.len());
    Ok((load_scene(renderer, &content, &spec)?, None))
}

/// With `--scene-sim N`, replace the stored matrices by the ones the Schedule produced.
fn apply_sim(
    opt: &Options,
    layouts: &Path,
    scene_path: &Path,
    data: &mut scene_view::SceneData,
) -> Result<()> {
    let Some(frames) = opt.scene_sim else {
        return Ok(());
    };
    let sim = scene_view::simulate(layouts, scene_path, frames)?;
    let (ok, missing) = sim.apply(data);
    println!(
        "scene-sim: {frames} fixed frames, world built in {:.0} ms, {ok} instances read back from the World ({missing} kept stored); transform writes frame 1: {}, last: {}; live steering candidates {}, EntityInput updates {}",
        sim.build_ms, sim.written_first, sim.written_last, sim.steering_live_candidates, sim.steering_input_updates
    );
    Ok(())
}

/// (differing pixels, max RGB channel delta, sum of RGB deltas, visual diff image).
fn diff_images(a: &[u8], b: &[u8]) -> (usize, u8, u64, Vec<u8>) {
    let mut differing = 0usize;
    let (mut max_delta, mut sum) = (0u8, 0u64);
    let mut diff = vec![255u8; a.len()];
    for ((a, b), d) in a.chunks(4).zip(b.chunks(4)).zip(diff.chunks_mut(4)) {
        let deltas = [0, 1, 2].map(|k| a[k].abs_diff(b[k]));
        let m = deltas.into_iter().max().unwrap_or(0);
        sum += deltas.iter().map(|&x| u64::from(x)).sum::<u64>();
        max_delta = max_delta.max(m);
        if m > 0 {
            differing += 1;
            d.copy_from_slice(&[255, 0, 0, 255]);
        } else {
            let g = (u16::from(a[0]) + u16::from(a[1]) + u16::from(a[2])) / 3 / 3 + 40;
            d.copy_from_slice(&[g as u8, g as u8, g as u8, 255]);
        }
    }
    (differing, max_delta, sum, diff)
}

/// Put the stored (pre-simulation) matrices back into the draw lists, in `all` order.
fn loaded_swap(loaded: &mut Loaded, data: &scene_view::SceneData, all: &[Instance]) {
    let mut at = 0;
    for (it, g) in loaded.items.iter_mut().zip(&data.groups) {
        it.instances = all[at..at + g.instances.len()].to_vec();
        at += g.instances.len();
    }
    if !data.markers.is_empty() {
        if let Some(it) = loaded.items.get_mut(data.groups.len()) {
            it.instances = all[at..].to_vec();
        }
    }
}

fn median(v: &[f64]) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    s.get(s.len() / 2).copied().unwrap_or(0.0)
}

/// Render the scene with stored matrices and with World-simulated matrices (same camera, same GPU
/// data) and report how the images differ. This is not a comparison with the original game.
fn compare_scene(opt: &Options, out: &Path) -> Result<()> {
    let scene_path = opt.scene.as_ref().context("--compare needs --scene")?;
    let frames = opt.scene_sim.unwrap_or(3).max(1);
    let renderer = pollster::block_on(Renderer::new_offscreen(opt.width, opt.height))?;
    let content = open_content(opt)?;
    let layouts = scene_view::layouts_path(opt.type_layouts.as_ref())?;
    let mut data = scene_view::collect(&content, &opt.data, &layouts, scene_path)?;
    anyhow::ensure!(
        !data.groups.is_empty() || !data.markers.is_empty(),
        "--compare: scene has no drawable instances; empty images are not a comparison"
    );
    let mut loaded = load_scene_view(&renderer, &content, &mut data)?;
    let mut renderer = renderer;
    let uniforms =
        scene_view::FlyCam::framing(data.bounds).uniforms(opt.width as f32 / opt.height as f32);
    let neutral = renderer.neutral_view();
    let mut shoot = |loaded: &Loaded| -> Result<(Vec<u8>, f64)> {
        let mut times = Vec::new();
        let mut img = Vec::new();
        for _ in 0..5 {
            let draws = loaded.draws(&neutral);
            let t = std::time::Instant::now();
            img = renderer.render_to_image(&uniforms, &draws)?.2;
            times.push(t.elapsed().as_secs_f64() * 1e3);
        }
        Ok((img, median(&times)))
    };
    let (stored_img, stored_ms) = shoot(&loaded)?;

    let stored_inst: Vec<Instance> = data
        .groups
        .iter()
        .flat_map(|g| g.instances.iter().copied())
        .chain(data.markers.iter().copied())
        .collect();
    let sim = scene_view::simulate(&layouts, scene_path, frames)?;
    let (read_back, missing) = sim.apply(&mut data);
    let sim_inst: Vec<Instance> = data
        .groups
        .iter()
        .flat_map(|g| g.instances.iter().copied())
        .chain(data.markers.iter().copied())
        .collect();
    let bits = |i: &Instance| i.model.map(|r| r.map(f32::to_bits));
    let matrices_differing = stored_inst
        .iter()
        .zip(&sim_inst)
        .filter(|(a, b)| bits(a) != bits(b))
        .count();
    let mut max_matrix_delta = 0f32;
    for (a, b) in stored_inst.iter().zip(&sim_inst) {
        for (ra, rb) in a.model.iter().zip(&b.model) {
            for (x, y) in ra.iter().zip(rb) {
                max_matrix_delta = max_matrix_delta.max((x - y).abs());
            }
        }
    }
    loaded.sync(&data);
    let (sim_img, sim_render_ms) = shoot(&loaded)?;

    let (w, h) = (opt.width as usize, opt.height as usize);
    let (differing, max_delta, sum, diff) = diff_images(&stored_img, &sim_img);
    // Close-up on the instance whose matrix moved the most, so a sub-pixel change at the framed
    // distance is looked at where it could show.
    let close_up = if max_matrix_delta > 0.0 {
        let (mut at, mut best) = (0usize, 0f32);
        for (k, (a, b)) in stored_inst.iter().zip(&sim_inst).enumerate() {
            let d = a
                .model
                .iter()
                .flatten()
                .zip(b.model.iter().flatten())
                .map(|(x, y)| (x - y).abs())
                .fold(0f32, f32::max);
            if d > best {
                (at, best) = (k, d);
            }
        }
        let t = sim_inst[at].model[3];
        let mut cam = scene_view::FlyCam::framing(data.bounds);
        cam.pitch = -0.5;
        cam.yaw = 0.0;
        let fw = cam.forward();
        cam.eye = [0, 1, 2].map(|k| t[k] - fw[k] * 3.0);
        cam.near = 0.05;
        cam.far = 500.0;
        let u = cam.uniforms(opt.width as f32 / opt.height as f32);
        loaded_swap(&mut loaded, &data, &stored_inst);
        let a = renderer.render_to_image(&u, &loaded.draws(&neutral))?.2;
        loaded.sync(&data);
        let b = renderer.render_to_image(&u, &loaded.draws(&neutral))?.2;
        let (n, mx, sm, d) = diff_images(&a, &b);
        let stem = out.with_extension("");
        write_png(
            &stem.with_extension("closeup.diff.png"),
            w as u32,
            h as u32,
            &d,
        )?;
        write_png(
            &stem.with_extension("closeup.sim.png"),
            w as u32,
            h as u32,
            &b,
        )?;
        Some(serde_json::json!({
            "camera_eye": cam.eye, "target_instance": at, "target_matrix_delta": best,
            "pixels_differing": n, "pixels_differing_percent": 100.0 * n as f64 / (w * h) as f64,
            "max_channel_delta": mx, "mean_abs_diff_per_channel": sm as f64 / ((w * h) as f64 * 3.0),
        }))
    } else {
        None
    };
    let px = (w * h) as f64;
    let stem = out.with_extension("");
    write_png(
        &stem.with_extension("stored.png"),
        w as u32,
        h as u32,
        &stored_img,
    )?;
    write_png(
        &stem.with_extension("sim.png"),
        w as u32,
        h as u32,
        &sim_img,
    )?;
    let diff_path = stem.with_extension("diff.png");
    write_png(&diff_path, w as u32, h as u32, &diff)?;
    let step_ms = &sim.frame_ms;
    let steady: Vec<f64> = step_ms.iter().skip(1).copied().collect();
    let report = serde_json::json!({
        "scene": scene_path.file_name().and_then(|n| n.to_str()),
        "kind": "stored original LocalToWorld vs Rust TransformSystem LocalToWorld, same camera, same GPU data",
        "not_a_comparison_with_the_original_game": true,
        "resolution": [w, h],
        "fixed_frames": frames,
        "instances": stored_inst.len(),
        "instances_read_back_from_world": read_back,
        "instances_missing_in_world": missing,
        "instances_with_different_matrix_bits": matrices_differing,
        "steering": {
            "types_present": sim.steering_types_present,
            "live_candidates": sim.steering_live_candidates,
            "entity_input_updates": sim.steering_input_updates,
            "scope": "shared steering only; no locomotion or collision systems",
        },
        "max_abs_matrix_element_delta": max_matrix_delta,
        "pixels": w * h,
        "pixels_differing": differing,
        "pixels_differing_percent": 100.0 * differing as f64 / px,
        "max_channel_delta": max_delta,
        "mean_abs_diff_per_channel": sum as f64 / (px * 3.0),
        "timing_ms": {
            "world_build": sim.build_ms,
            "schedule_first_frame": step_ms.first().copied().unwrap_or(0.0),
            "schedule_steady_median": if steady.is_empty() { serde_json::Value::Null } else { median(&steady).into() },
            "transform_entities_written_first_frame": sim.written_first,
            "transform_entities_written_last_frame": sim.written_last,
            "render_plus_readback_stored_median_of_5": stored_ms,
            "render_plus_readback_sim_median_of_5": sim_render_ms,
        },
        "close_up_at_max_delta_instance": close_up,
        "images": {"diff": diff_path, "stored": stem.with_extension("stored.png"), "sim": stem.with_extension("sim.png")},
    });
    std::fs::write(out, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
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
    if let Some(scene_path) = &opt.scene {
        let content = open_content(opt)?;
        let layouts = scene_view::layouts_path(opt.type_layouts.as_ref())?;
        let mut data = scene_view::collect(&content, &opt.data, &layouts, scene_path)?;
        apply_sim(opt, &layouts, scene_path, &mut data)?;
        let loaded = load_scene_view(&renderer, &content, &mut data)?;
        println!("{}", data.stats.report());
        let mut renderer = renderer;
        let cam = scene_view::FlyCam::framing(data.bounds);
        let uniforms = cam.uniforms(opt.width as f32 / opt.height as f32);
        let neutral = renderer.neutral_view();
        let draws = loaded.draws(&neutral);
        let (w, h, rgba) = renderer.render_to_image(&uniforms, &draws)?;
        let path = opt
            .offscreen
            .clone()
            .unwrap_or_else(|| PathBuf::from("scene.png"));
        write_png(&path, w, h, &rgba)?;
        let clear = (rgba[0], rgba[1], rgba[2]);
        let covered = rgba
            .chunks(4)
            .filter(|p| (p[0], p[1], p[2]) != clear)
            .count();
        println!(
            "wrote {} ({w}x{h}, {} draw groups, {} textures bound, {covered} pixels covered)",
            path.display(),
            loaded.items.len(),
            loaded.textures_bound
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
    if opt.scene.is_some() {
        return export_scene_web(opt, path);
    }
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
            instances: vec![],
            texture,
            texture_index: None,
            cutout: false,
        });
    }
    let snapshot = Snapshot {
        version: 1,
        description: "Original art in a showcase grid; no gameplay or original world placement"
            .into(),
        uniforms: frame_uniforms(opt.width as f32 / opt.height as f32),
        items,
        textures: vec![],
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

/// Export an original scene's placements (instanced) for the browser viewer. Local disk only.
fn export_scene_web(opt: &Options, path: &Path) -> Result<()> {
    use rrising_render::snapshot::{Snapshot, SnapshotItem};
    let scene_path = opt.scene.as_ref().context("--scene")?;
    let content = open_content(opt)?;
    let layouts = scene_view::layouts_path(opt.type_layouts.as_ref())?;
    let mut data = scene_view::collect(&content, &opt.data, &layouts, scene_path)?;
    apply_sim(opt, &layouts, scene_path, &mut data)?;
    let mut pool = Vec::new();
    let mut slot: std::collections::HashMap<unity_formats::player::ObjRef, Option<usize>> =
        Default::default();
    let mut items = Vec::new();
    let identity = Instance::from_matrix(&float4x4::identity);
    let mut grey: Option<usize> = None;
    let mut array_textures: std::collections::HashMap<unity_formats::player::ObjRef, bool> =
        Default::default();
    let mut array_instances = 0;
    let (mut failed_instances, mut failed_textures) = (0, 0);
    for g in &data.groups {
        let real = match &g.texture {
            None => None,
            Some(r) => *slot.entry(r.clone()).or_insert_with(|| {
                let exported = scene_view::read_base_map(&content, r).and_then(|(t, layer0)| {
                    array_textures.insert(r.clone(), layer0);
                    rrising_render::unity_texture::export(&t)
                });
                match exported {
                    Ok(t) => {
                        pool.push(t);
                        Some(pool.len() - 1)
                    }
                    Err(e) => {
                        log::warn!("base map {r:?} not exported: {e:#}");
                        failed_textures += 1;
                        None
                    }
                }
            }),
        };
        if g.texture.is_some() && real.is_none() {
            failed_instances += g.instances.len();
        }
        if real.is_some()
            && g.texture
                .as_ref()
                .is_some_and(|r| array_textures.get(r) == Some(&true))
        {
            array_instances += g.instances.len();
        }
        let texture_index = real.or_else(|| {
            Some(*grey.get_or_insert_with(|| {
                pool.push(rrising_render::texture::TextureData::flat(
                    "fallback grey",
                    [128, 128, 128, 255],
                ));
                pool.len() - 1
            }))
        });
        items.push(SnapshotItem {
            name: g.name.clone(),
            vertices: (*g.verts).clone(),
            indices: (*g.inds).clone(),
            transform: identity,
            instances: g.instances.clone(),
            texture: None,
            texture_index,
            cutout: g.cutout,
        });
    }
    data.stats.texture_fallback_instances = failed_instances;
    data.stats.texture_fallback_textures = failed_textures;
    data.stats.array_layer0_instances = array_instances;
    println!("{}", data.stats.report());
    if !data.markers.is_empty() {
        let (vertices, indices) = scene_view::marker_box();
        items.push(SnapshotItem {
            name: "marker box".into(),
            vertices,
            indices,
            transform: identity,
            instances: data.markers.clone(),
            texture: None,
            texture_index: None,
            cutout: false,
        });
    }
    let cam = scene_view::FlyCam::framing(data.bounds);
    let snapshot = Snapshot {
        version: 1,
        description: "Original scene entities at original LocalToWorld placements; not gameplay, no lighting parity".into(),
        uniforms: cam.uniforms(opt.width as f32 / opt.height as f32),
        items,
        textures: pool,
    };
    let mut writer = std::io::BufWriter::new(
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?,
    );
    serde_json::to_writer(&mut writer, &snapshot)?;
    use std::io::Write;
    writer.flush()?;
    println!(
        "exported {} draw groups, {} pooled textures to {} (local original content; excluded from Git)",
        snapshot.items.len(),
        snapshot.textures.len(),
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
    let mut material_mesh_samples = Vec::new();
    for (index, chunk) in resolved.chunks.iter().enumerate() {
        for entity in 0..chunk.header.count as usize {
            if material_mesh_samples.len() >= 8 {
                break;
            }
            if let Some(bytes) = resolved.component_bytes(index, entity, 0x3ae3_a524_8a68_9306)? {
                let info = rrising_ecs::MaterialMeshInfo::from_bytes(bytes)?;
                material_mesh_samples.push(serde_json::json!({
                    "chunk": index, "entity_in_chunk": entity,
                    "material_raw": info.material, "mesh_raw": info.mesh,
                    "submesh": info.sub_mesh(), "range": info.material_mesh_index_range(),
                    "runtime_material": info.is_runtime_material(), "runtime_mesh": info.is_runtime_mesh(),
                    "material_array_index": info.material_array_index(), "mesh_array_index": info.mesh_array_index()
                }));
            }
        }
        if material_mesh_samples.len() >= 8 {
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
        "material_mesh_samples": material_mesh_samples,
        "limitation": "Recovered storage with chunk-boundary checks; not a full TypeInfo oracle or game world"
    });
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

/// Median of a non-empty list of durations, in milliseconds.
fn median_ms(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// `--bench`: wall-clock timings of the Rust port only (there is no original-game comparison).
/// Run with `--release`; numbers from a debug build are meaningless.
fn bench(opt: &Options) -> Result<()> {
    use std::time::Instant;
    let dir = opt.data.join("StreamingAssets/ContentArchives");
    let catalog_path = dir.join("archive_dependencies.bin");
    let t = Instant::now();
    let catalog = rrising_content::catalog::RuntimeContentCatalog::open(&catalog_path)?;
    let catalog_s = t.elapsed().as_secs_f64();
    let paths = catalog.archive_paths(&dir);
    let take = opt.archives.min(paths.len());
    let t = Instant::now();
    let content = GameContent::open(&opt.data, &paths[..take], &opt.tpk)?;
    let index_s = t.elapsed().as_secs_f64();
    let objects: usize = content.class_counts().values().sum();

    let scenes_dir = opt.data.join("StreamingAssets/EntityScenes");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&scenes_dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "entities"))
        .collect();
    files.sort();
    let t = Instant::now();
    let (mut ok, mut bytes, mut chunks) = (0usize, 0usize, 0usize);
    for f in &files {
        let Ok(scene) = rrising_content::entity_scenes::SceneFile::open(f) else {
            continue;
        };
        bytes += scene.bytes().len();
        let decoded = scene.chunks().map(|c| c.len());
        let all = decoded.is_ok()
            & scene.archetypes().is_ok()
            & scene.buffer_data().is_ok()
            & scene.enabled_bits().is_ok()
            & scene.shared_and_managed_components().is_ok()
            & scene.blob_assets().is_ok();
        chunks += decoded.unwrap_or(0);
        ok += usize::from(all);
    }
    let scenes_s = t.elapsed().as_secs_f64();

    let renderer = pollster::block_on(Renderer::new_offscreen(opt.width, opt.height))?;
    let spec = scene::showcase(&content, opt.items);
    let loaded = load_scene(&renderer, &content, &spec)?;
    let mut renderer = renderer;
    let uniforms = frame_uniforms(opt.width as f32 / opt.height as f32);
    let neutral = renderer.neutral_view();
    let draws = loaded.draws(&neutral);
    renderer.render_to_image(&uniforms, &draws)?; // warm-up
    let mut times = Vec::with_capacity(opt.frames);
    for _ in 0..opt.frames {
        let t = Instant::now();
        renderer.render_to_image(&uniforms, &draws)?;
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    let min = times.iter().cloned().fold(f64::MAX, f64::min);
    let median = median_ms(times);
    println!("bench (Rust port only; no original-game comparison)");
    println!(
        "  catalog load:        {catalog_s:.3} s ({} physical archives)",
        paths.len()
    );
    println!("  archive indexing:    {index_s:.2} s ({take} archives, {objects} objects)");
    println!(
        "  scene decode:        {scenes_s:.2} s ({ok}/{} scenes fully decoded, {chunks} chunks, {:.1} MiB)",
        files.len(),
        bytes as f64 / 1048576.0
    );
    println!(
        "  offscreen frame:     median {median:.2} ms, min {min:.2} ms over {} frames at {}x{}, {} items (includes GPU readback)",
        opt.frames,
        opt.width,
        opt.height,
        loaded.items.len()
    );
    Ok(())
}

fn main() -> Result<()> {
    env_logger::init();
    let opt = parse_args()?;
    if let Some(path) = &opt.inspect_save {
        let save = PersistentSave::open(path)?;
        let snapshot = SavedSnapshot::read(&save)?;
        let header = snapshot.header();
        let world = snapshot.world_data();
        let world_stats = world.stats();
        let systems = snapshot.systems();
        let projection = if opt.restore_projection {
            use rrising_content::{
                prefab_catalog::ScenePrefabCatalog, restore_world::RestoredWorldProjection,
                type_layouts::TypeLayouts,
            };
            let path = opt
                .type_layouts
                .as_ref()
                .context("missing server layout report")?;
            let layouts = TypeLayouts::from_server_json(&std::fs::read(path)?)?;
            let catalogs = ScenePrefabCatalog::read_server_game_data(&opt.data, &layouts)?;
            let restored = RestoredWorldProjection::build(&snapshot, &layouts, &catalogs)?;
            Some(serde_json::json!({
                "live_entities": restored.bindings.len() - 1,
                "prefab_roots": restored.prefabs.len(),
                "scalar_values": restored.copied_scalar_values,
                "cleared_references": restored.cleared_references,
            }))
        } else {
            None
        };
        let linked_entities: usize = header.prefabs.iter().map(|p| p.linked_entities.len()).sum();
        let members: Vec<_> = save
            .members()
            .iter()
            .map(|m| {
                serde_json::json!({
                    "name": m.name, "offset": m.offset, "size": m.size,
                })
            })
            .collect();
        let mut serialization_jobs = Vec::new();
        for (member_index, job) in snapshot.jobs() {
            let member = &save.members()[*member_index];
            let stats = job.stats();
            serialization_jobs.push(serde_json::json!({
                "member_name": member.name, "bytes": member.size,
                "archetypes": stats.archetypes, "chunks": stats.chunks,
                "entities": stats.entities, "columns": stats.columns,
                "buffer_entities": stats.buffer_entities,
            }));
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "kind": "save-container-inspection", "container_version": CONTAINER_VERSION,
                "uncompressed_bytes": save.uncompressed_bytes(),
                "table_bytes": save.table_bytes(), "members": members,
                "serialization_jobs": serialization_jobs,
                "indexed_entity_count": snapshot.entity_count(),
                "ecs_projection": projection,
                "world_data": {
                    "entity_capacity": world.entity_capacity(),
                    "debug_archetypes": world_stats.archetypes,
                    "debug_chunks": world_stats.chunks,
                    "debug_entities": world_stats.entities,
                    "debug_type_hashes": world_stats.type_hashes,
                },
                "systems": {
                    "elapsed_time": systems.elapsed_time,
                    "world_frame": systems.world_frame()?,
                    "slot_count": systems.slot_count,
                    "records": systems.records.iter().map(|record| serde_json::json!({
                        "slot": record.slot_index, "identifier": record.identifier,
                        "payload_offset": record.payload_offset, "payload_bytes": record.payload.len(),
                        "payload_bits": record.payload_bits,
                    })).collect::<Vec<_>>(),
                },
                "header": {
                    "serialization_format_version": header.serialization_format_version,
                    "persistence_version": header.persistence_version,
                    "game_version": header.game_version,
                    "application_type": header.application_type,
                    "conversion_target": header.conversion_target,
                    "prefab_conversion_method": header.prefab_conversion_method,
                    "using_server_only_subscenes": header.using_server_only_subscenes,
                    "prefab_count": header.prefabs.len(), "linked_entity_count": linked_entities,
                    "persistent_component_count": header.persistent_components.len(),
                    "required_component_count": header.required_components.len(),
                    "blob_count": header.blobs.len(),
                },
                "world_restored": false,
            }))?
        );
        return Ok(());
    }
    if opt.bench {
        return bench(&opt);
    }
    if let Some(path) = &opt.inspect_scene {
        return inspect_scene(&opt, path);
    }
    if let Some(path) = &opt.compare {
        return compare_scene(&opt, path);
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

#[cfg(test)]
mod tests {
    use super::diff_images;

    #[test]
    fn diff_counts_pixels_and_channel_deltas() {
        let a = vec![10u8; 4 * 4];
        let mut b = a.clone();
        assert_eq!(diff_images(&a, &b).0, 0);
        b[4] = 14; // one pixel, red +4
        b[9] = 5; // another pixel, green -5
        let (n, max, sum, img) = diff_images(&a, &b);
        assert_eq!((n, max, sum), (2, 5, 9));
        assert_eq!(&img[4..8], &[255, 0, 0, 255]);
    }
}
