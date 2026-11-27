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

mod input_control;
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
    list_input_entities: Option<PathBuf>,
    inspect_save: Option<PathBuf>,
    clone_prefab: Option<i32>,
    restore_projection: bool,
    scene: Option<PathBuf>,
    type_layouts: Option<PathBuf>,
    /// `--scene-sim N`: run fixed steering/transform frames and draw the World's LocalToWorld.
    scene_sim: Option<usize>,
    scene_live: bool,
    control_entity: Option<rrising_ecs::Entity>,
    spawn_player: bool,
    player_position: Option<[f32; 3]>,
    animate_player: bool,
    experimental_motion: bool,
    follow_player: bool,
    diagnostic_hud: bool,
    terrain_chunk: Option<u16>,
    terrain_neighbors: Vec<u16>,
    server_data: Option<PathBuf>,
    server_layouts: Option<PathBuf>,
    animation_frame: Option<usize>,
    /// `--compare out.json`: stored-vs-simulated image comparison for `--scene`.
    compare: Option<PathBuf>,
    bench: bool,
    frames: usize,
}

fn parse_player_position(value: &str) -> Result<[f32; 3]> {
    let values: Vec<f32> = value
        .split(',')
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()
        .context("--player-position needs x,y,z")?;
    let position: [f32; 3] = values
        .try_into()
        .map_err(|_| anyhow::anyhow!("--player-position needs exactly three coordinates"))?;
    anyhow::ensure!(
        position.iter().all(|v| v.is_finite()),
        "--player-position coordinates must be finite"
    );
    Ok(position)
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
        list_input_entities: None,
        inspect_save: None,
        clone_prefab: None,
        restore_projection: false,
        scene: None,
        type_layouts: None,
        scene_sim: None,
        scene_live: false,
        control_entity: None,
        spawn_player: false,
        player_position: None,
        animate_player: false,
        experimental_motion: false,
        follow_player: false,
        diagnostic_hud: false,
        terrain_chunk: None,
        terrain_neighbors: Vec::new(),
        server_data: None,
        server_layouts: None,
        animation_frame: None,
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
            "--list-input-entities" => opt.list_input_entities = Some(PathBuf::from(take()?)),
            "--inspect-save" => opt.inspect_save = Some(PathBuf::from(take()?)),
            "--clone-prefab" => {
                opt.clone_prefab = Some(take()?.parse().context("expected a signed prefab GUID")?)
            }
            "--restore-projection" => opt.restore_projection = true,
            "--scene" => opt.scene = Some(PathBuf::from(take()?)),
            "--scene-sim" => {
                opt.scene_sim = Some(take()?.parse().context("--scene-sim takes a frame count")?)
            }
            "--scene-live" => opt.scene_live = true,
            "--spawn-player" => opt.spawn_player = true,
            "--player-position" => opt.player_position = Some(parse_player_position(&take()?)?),
            "--animate-player" => opt.animate_player = true,
            "--experimental-motion" => opt.experimental_motion = true,
            "--follow-player" => opt.follow_player = true,
            "--diagnostic-hud" => opt.diagnostic_hud = true,
            "--terrain-chunk" => {
                let value = take()?;
                let (x, z) = value.split_once(',').context("--terrain-chunk needs x,z")?;
                let x: i8 = x.parse()?;
                let z: i8 = z.parse()?;
                opt.terrain_chunk = Some(u16::from_le_bytes([x as u8, z as u8]));
            }
            "--terrain-neighbor" => {
                let value = take()?;
                let (x, z) = value
                    .split_once(',')
                    .context("--terrain-neighbor needs x,z")?;
                opt.terrain_neighbors.push(u16::from_le_bytes([
                    x.parse::<i8>()? as u8,
                    z.parse::<i8>()? as u8,
                ]));
            }
            "--server-data" => opt.server_data = Some(PathBuf::from(take()?)),
            "--server-layouts" => opt.server_layouts = Some(PathBuf::from(take()?)),
            "--animation-frame" => {
                opt.animation_frame = Some(
                    take()?
                        .parse()
                        .context("--animation-frame takes an index")?,
                )
            }
            "--control-entity" => {
                let value = take()?;
                let (index, version) = value
                    .split_once(':')
                    .context("--control-entity needs index:version")?;
                opt.control_entity =
                    Some(rrising_ecs::Entity::new(index.parse()?, version.parse()?));
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
        opt.clone_prefab.is_none() || (opt.inspect_save.is_none() && opt.type_layouts.is_some()),
        "--clone-prefab requires --type-layouts and server data via --data"
    );
    anyhow::ensure!(
        opt.scene_sim.is_none() || (opt.scene.is_some() && opt.scene_sim != Some(0)),
        "--scene-sim needs --scene and a positive frame count"
    );
    anyhow::ensure!(!opt.scene_live || (opt.scene.is_some() && opt.scene_sim.is_none() && opt.offscreen.is_none() && opt.export_web.is_none() && opt.compare.is_none()), "--scene-live needs --scene and a window; it cannot be combined with --scene-sim or export modes");
    anyhow::ensure!(
        opt.control_entity.is_none() || opt.scene_live,
        "--control-entity requires --scene-live"
    );
    anyhow::ensure!(
        !opt.spawn_player || ((opt.scene_live || (opt.scene.is_some() && (opt.offscreen.is_some() || opt.export_web.is_some()))) && opt.control_entity.is_none() && opt.scene_sim.is_none()),
        "--spawn-player requires --scene-live or --scene with --offscreen/--export-web, without --control-entity or --scene-sim"
    );
    anyhow::ensure!(
        opt.player_position.is_none() || opt.spawn_player,
        "--player-position requires --spawn-player"
    );
    anyhow::ensure!(
        !opt.animate_player || (opt.spawn_player && opt.scene_live),
        "--animate-player requires --scene-live --spawn-player"
    );
    anyhow::ensure!(
        !opt.experimental_motion || (opt.spawn_player && opt.scene_live),
        "--experimental-motion requires --scene-live --spawn-player"
    );
    anyhow::ensure!(
        !opt.follow_player || (opt.spawn_player && opt.scene_live),
        "--follow-player requires --scene-live --spawn-player"
    );
    anyhow::ensure!(
        !opt.diagnostic_hud
            || opt.spawn_player
            || (opt.scene.is_some() && opt.export_web.is_some()),
        "--diagnostic-hud requires --spawn-player or a scene export"
    );
    anyhow::ensure!(opt.terrain_chunk.is_none() || (opt.scene.is_some() && opt.server_data.is_some() && opt.server_layouts.is_some() && !opt.scene_live && opt.scene_sim.is_none() && !opt.spawn_player && opt.export_web.is_none() && opt.compare.is_none()), "--terrain-chunk requires --scene, --server-data and --server-layouts and supports static native viewing only");
    anyhow::ensure!(
        opt.terrain_neighbors.is_empty() || opt.terrain_chunk.is_some(),
        "--terrain-neighbor requires --terrain-chunk"
    );
    anyhow::ensure!(
        opt.animation_frame.is_none()
            || (opt.spawn_player
                && (opt.offscreen.is_some() || opt.export_web.is_some())
                && opt.animation_frame.is_some_and(|n| n < 24)),
        "--animation-frame needs --spawn-player and --offscreen/--export-web with an index from 0 to 23"
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
    hud: Option<DiagnosticHud>,
}

/// Static frame art only; no inventory, hotkeys or Unity Canvas behavior.
struct DiagnosticHud {
    texture: rrising_render::GpuTexture,
    layout: rrising_render::overlay::ActionBarLayout,
}

fn action_bar_layout(
    source: &rrising_content::hud::ActionBarFrame,
) -> rrising_render::overlay::ActionBarLayout {
    rrising_render::overlay::ActionBarLayout {
        slot_size: source.slot_size,
        frame_extra: source.frame_extra,
        anchor_position: source.anchor_position,
        spacing: source.spacing,
        count: source.count,
    }
}

impl DiagnosticHud {
    fn load(renderer: &Renderer, content: &GameContent) -> Result<Self> {
        let source = rrising_content::hud::candidate_action_bar(content)?;
        let texture = renderer.upload_texture(&content.read_texture(&source.texture)?)?;
        let layout = action_bar_layout(&source);
        layout.validate()?;
        Ok(Self { texture, layout })
    }
    fn rects(&self, width: u32, height: u32) -> Vec<[f32; 4]> {
        self.layout.rects(width, height)
    }
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
        hud: None,
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
        hud: None,
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
    pending_player_keys: std::collections::HashSet<winit::keyboard::KeyCode>,
    last_tick: Option<std::time::Instant>,
    live: Option<(scene_view::SimWorld, scene_view::SceneData)>,
    live_tick: Option<std::time::Instant>,
    control: Option<(rrising_ecs::Entity, rrising_ecs::TypeIndex)>,
    input_direction: [f32; 3],
    player_animation: Option<PlayerAnimation>,
    follow_player: bool,
    follow_previous: Option<[f32; 3]>,
}

struct PlayerAnimation {
    duration: f32,
    frames: Vec<Vec<Vec<rrising_render::mesh::Vertex>>>,
    rest: Vec<Arc<Vec<rrising_render::mesh::Vertex>>>,
    first_group: usize,
    started: std::time::Instant,
    motion: Option<MotionAnimation>,
    // Outer None means no pose has yet been uploaded; inner None is the rest pose.
    last_frame: Option<Option<usize>>,
}

/// Diagnostic visual policy, not the original Animator's state machine.
struct MotionAnimation {
    previous: [f32; 3],
    phase_seconds: f32,
    moving: bool,
}

impl MotionAnimation {
    fn advance(&mut self, position: [f32; 3], elapsed: f32, duration: f32) {
        self.moving = position[0] != self.previous[0] || position[2] != self.previous[2];
        if self.moving {
            log::debug!(target: "rrising::input", "player moved {:?} -> {position:?}", self.previous);
        }
        self.previous = position;
        if self.moving {
            self.phase_seconds = (self.phase_seconds + elapsed).rem_euclid(duration);
        }
    }

    fn frame(&self, duration: f32, count: usize) -> Option<usize> {
        self.moving
            .then(|| ((self.phase_seconds / duration * count as f32) as usize).min(count - 1))
    }
}

impl App {
    fn frame(&mut self) {
        if self.renderer.is_none() || self.loaded.is_none() {
            return;
        }
        let (w, h) = match &self.window {
            Some(win) => {
                let s = win.inner_size();
                (s.width.max(1), s.height.max(1))
            }
            None => (1280, 720),
        };
        if let (Some((sim, scene)), Some(loaded)) = (&mut self.live, &mut self.loaded) {
            if let Some((entity, ty)) = self.control {
                // The viewer has a free camera, not the game's camera state. Use its yaw as
                // an explicit experimental camera projection and the captured mode 0.
                let yaw = self.fly.as_ref().map_or(0.0, |camera| camera.yaw);
                let step_keys = input_control::step_keys(&self.keys, &self.pending_player_keys);
                if let Err(error) = input_control::write_input(
                    &mut sim.world,
                    entity,
                    ty,
                    &step_keys,
                    yaw,
                    &mut self.input_direction,
                ) {
                    log::error!("player input: {error:#}");
                    self.control = None;
                }
            }
            let now = std::time::Instant::now();
            let elapsed = self.live_tick.replace(now).map_or(0, |last| {
                now.duration_since(last)
                    .as_nanos()
                    .min(u128::from(u64::MAX)) as u64
            });
            match sim.advance(elapsed, 4) {
                Ok(steps) if steps > 0 => {
                    self.pending_player_keys.clear();
                    sim.apply(scene);
                    loaded.sync(scene);
                    if let (Some((entity, _)), Some(animation)) =
                        (self.control, &mut self.player_animation)
                    {
                        if let (Some(motion), Some(instance)) =
                            (&mut animation.motion, sim.instance(entity))
                        {
                            let p = instance.model[3];
                            motion.advance(
                                [p[0], p[1], p[2]],
                                steps as f32 * scene_view::STEP_NS as f32 * 1e-9,
                                animation.duration,
                            );
                        }
                    }
                    if self.follow_player {
                        if let (Some((entity, _)), Some(camera)) = (self.control, &mut self.fly) {
                            if let Some(instance) = sim.instance(entity) {
                                let p = instance.model[3];
                                follow_camera(
                                    camera,
                                    &mut self.follow_previous,
                                    [p[0], p[1], p[2]],
                                );
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    log::error!("scene simulation: {error:#}");
                    self.live = None;
                }
            }
        }
        if let (Some(animation), Some(loaded), Some(renderer)) =
            (&mut self.player_animation, &mut self.loaded, &self.renderer)
        {
            let count = animation.frames[0].len();
            let frame = if let Some(motion) = &animation.motion {
                motion.frame(animation.duration, count)
            } else {
                let phase = animation
                    .started
                    .elapsed()
                    .as_secs_f32()
                    .rem_euclid(animation.duration)
                    / animation.duration;
                Some(((phase * count as f32) as usize).min(count - 1))
            };
            if animation.last_frame != Some(frame) {
                for (part, frames) in animation.frames.iter().enumerate() {
                    let mesh = loaded.items[animation.first_group + part].mesh;
                    let vertices =
                        frame.map_or(animation.rest[part].as_slice(), |index| &frames[index]);
                    if let Err(error) = rrising_render::mesh::update_vertices(
                        renderer.queue(),
                        &loaded.meshes[mesh],
                        vertices,
                    ) {
                        log::error!("experimental animation: {error:#}");
                    }
                }
                animation.last_frame = Some(frame);
            }
        }
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
                    if self.control.is_some() {
                        0.
                    } else {
                        (k(K::KeyW) - k(K::KeyS)) * boost
                    },
                    if self.control.is_some() {
                        0.
                    } else {
                        (k(K::KeyD) - k(K::KeyA)) * boost
                    },
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
        let (Some(r), Some(l)) = (&mut self.renderer, &self.loaded) else {
            return;
        };
        let neutral = r.neutral_view();
        let draws = l.draws(&neutral);
        let rects = l.hud.as_ref().map_or_else(Vec::new, |hud| hud.rects(w, h));
        let overlay: Vec<_> = l
            .hud
            .as_ref()
            .map(|hud| rrising_render::overlay::OverlayDraw {
                texture: &hud.texture.view,
                rects: &rects,
                uv: [0.0, 0.0, 1.0, 1.0],
            })
            .into_iter()
            .collect();
        if let Err(e) = r.render_surface_with_overlay(&uniforms, &draws, &overlay) {
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
            Ok((loaded, fly, live, control, animation, follow)) => {
                self.follow_previous = control
                    .and_then(|(entity, _)| live.as_ref()?.0.instance(entity))
                    .map(|i| [i.model[3][0], i.model[3][1], i.model[3][2]]);
                self.follow_player = follow;
                self.fly = fly;
                self.live = live;
                self.control = control;
                self.player_animation = animation;
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
                    log::debug!(target: "rrising::input", "key {code:?} {:?}", event.state);
                    if self.control.is_some() {
                        input_control::record_player_key(
                            &mut self.keys,
                            &mut self.pending_player_keys,
                            code,
                            event.state,
                        );
                    } else {
                        input_control::record_key(&mut self.keys, code, event.state);
                    }
                }
            }
            WindowEvent::Focused(false) => {
                self.keys.clear();
                self.pending_player_keys.clear();
                self.last_tick = None;
                self.live_tick = None;
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

type LiveScene = Option<(scene_view::SimWorld, scene_view::SceneData)>;
type ControlTarget = Option<(rrising_ecs::Entity, rrising_ecs::TypeIndex)>;

fn place_terrain(
    opt: &Options,
    content: &GameContent,
    layouts_path: &Path,
    scene_path: &Path,
    data: &mut scene_view::SceneData,
) -> Result<()> {
    let Some(chunk) = opt.terrain_chunk else {
        return Ok(());
    };
    let layouts = rrising_content::type_layouts::TypeLayouts::from_server_json(&std::fs::read(
        opt.server_layouts.as_ref().context("server layouts")?,
    )?)?;
    let catalog = rrising_content::static_transforms::StaticTransformCatalog::read_server(
        opt.server_data.as_ref().context("server data")?,
        &layouts,
    )?;
    let source = catalog.source(chunk).context("terrain chunk not found")?;
    let name = scene_path
        .file_name()
        .and_then(|s| s.to_str())
        .context("scene filename")?;
    let guid = rrising_ecs::Hash128::from_guid(name.split('.').next().context("scene GUID")?);
    anyhow::ensure!(
        guid == source.scene,
        "scene GUID does not match terrain chunk source"
    );
    let matrix = catalog
        .placement_matrix(chunk)
        .context("terrain placement")?;
    scene_view::place_terrain_section(data, matrix);
    let mut seen = std::collections::BTreeSet::from([chunk]);
    let directory = opt.data.join("StreamingAssets/EntityScenes");
    for &neighbor in &opt.terrain_neighbors {
        anyhow::ensure!(
            seen.insert(neighbor),
            "duplicate terrain chunk {neighbor:#06x}"
        );
        let source = catalog
            .source(neighbor)
            .context("neighbor terrain chunk not found")?;
        let mut matches = Vec::new();
        for entry in std::fs::read_dir(&directory)? {
            let path = entry?.path();
            let Some(name) = path.file_name().and_then(|v| v.to_str()) else {
                continue;
            };
            if name.ends_with(".0.entities")
                && rrising_ecs::Hash128::from_guid(name.split('.').next().unwrap_or(""))
                    == source.scene
            {
                matches.push(path);
            }
        }
        anyhow::ensure!(
            matches.len() == 1,
            "neighbor scene missing or ambiguous for {neighbor:#06x}"
        );
        let mut next = scene_view::collect(content, &opt.data, layouts_path, &matches[0])?;
        scene_view::place_terrain_section(
            &mut next,
            catalog
                .placement_matrix(neighbor)
                .context("neighbor placement")?,
        );
        scene_view::append_static_section(data, next);
        println!(
            "Placed neighbor {neighbor:#06x}, yaw {} degrees",
            source.rotation_degrees
        );
    }
    println!(
        "Placed terrain chunk {chunk:#06x}, yaw {} degrees; experimental static placement.",
        source.rotation_degrees
    );
    Ok(())
}
type WindowedScene = (
    Loaded,
    Option<scene_view::FlyCam>,
    LiveScene,
    ControlTarget,
    Option<PlayerAnimation>,
    bool,
);

/// Translate the diagnostic camera by the player's world displacement, preserving
/// its current offset. This does not reproduce V Rising camera behavior.
fn follow_camera(
    camera: &mut scene_view::FlyCam,
    previous: &mut Option<[f32; 3]>,
    position: [f32; 3],
) {
    if !position.iter().all(|v| v.is_finite()) {
        return;
    }
    if let Some(old) = *previous {
        for axis in 0..3 {
            camera.eye[axis] += position[axis] - old[axis];
        }
    }
    *previous = Some(position);
}

fn player_camera(bounds: ([f32; 3], [f32; 3]), instance: Instance) -> scene_view::FlyCam {
    let mut camera = scene_view::FlyCam::framing(bounds);
    let position = instance.model[3];
    // Diagnostic framing informed by the supplied gameplay screenshot; the
    // original camera's parameters and collision behavior are not recovered.
    camera.eye = [position[0], position[1] + 10.0, position[2] - 10.0];
    camera.yaw = 0.0;
    camera.pitch = -0.75;
    camera.speed = 4.0;
    camera.near = 0.05;
    camera
}
fn run_windowed(renderer: &Renderer) -> Result<WindowedScene> {
    let opt = parse_args()?;
    let content = open_content(&opt)?;
    if let Some(scene_path) = &opt.scene {
        let layouts = scene_view::layouts_path(opt.type_layouts.as_ref())?;
        let mut data = scene_view::collect(&content, &opt.data, &layouts, scene_path)?;
        apply_sim(&opt, &layouts, scene_path, &mut data)?;
        place_terrain(&opt, &content, &layouts, scene_path, &mut data)?;
        let mut fly = Some(scene_view::FlyCam::framing(data.bounds));
        let mut sim = if opt.scene_live {
            println!(
                "Experimental live ECS scene: no player locomotion or gameplay animation system."
            );
            Some(scene_view::simulate(&layouts, scene_path, 0)?)
        } else {
            None
        };
        let mut animation = None;
        let selected_entity = if opt.spawn_player {
            let sim = sim.as_mut().context("spawn needs live world")?;
            let entity = scene_view::spawn_player(&mut sim.world, &opt.data, &layouts)?;
            if let Some(position) = opt.player_position {
                scene_view::set_diagnostic_player_position(&mut sim.world, entity, position)?;
                println!("Diagnostic player position {position:?}: not an original spawn, collision unchecked.");
            }
            if opt.experimental_motion {
                sim.enable_experimental_player_motion(entity)?;
                println!("Experimental motion enabled: approximate ordering, no collision, abilities, networking, or original scheduling.");
            }
            sim.advance(16_666_667, 1)?;
            let instance = sim
                .instance(entity)
                .context("spawned player's LocalToWorld unavailable")?;
            fly = Some(player_camera(data.bounds, instance));
            let first_group = data.groups.len();
            let parts = scene_view::attach_player_visual(&content, &mut data, entity, instance)?;
            if opt.animate_player {
                let (duration, frames) = scene_view::player_walk_samples(&content, 24)?;
                anyhow::ensure!(
                    parts == frames.len(),
                    "animation parts differ from visual parts"
                );
                animation = Some(PlayerAnimation {
                    duration,
                    frames,
                    rest: data.groups[first_group..first_group + parts]
                        .iter()
                        .map(|group| Arc::clone(&group.verts))
                        .collect(),
                    first_group,
                    started: std::time::Instant::now(),
                    motion: opt.experimental_motion.then_some(MotionAnimation {
                        previous: [
                            instance.model[3][0],
                            instance.model[3][1],
                            instance.model[3][2],
                        ],
                        phase_seconds: 0.0,
                        moving: false,
                    }),
                    last_frame: None,
                });
                if opt.experimental_motion {
                    println!("Experimental movement-driven rest/walk selection; not the game's Animator.");
                } else {
                    println!("Experimental looping walk samples; not the game's Animator.");
                }
            }
            println!("Experimental player visual: {parts} parts; first face is diagnostic and attachment behavior is unverified.");
            Some(entity)
        } else {
            opt.control_entity
        };
        let control = if let Some(entity) = selected_entity {
            let sim = sim.as_ref().context("control needs live world")?;
            let ty = sim
                .world
                .types
                .type_index_of("ProjectM.EntityInput")
                .context("scene has no EntityInput type")?;
            let query = rrising_ecs::EntityQuery {
                all: vec![ty],
                ..Default::default()
            };
            anyhow::ensure!(
                sim.world.store.query_entities(&query)?.contains(&entity),
                "control entity is not a live non-prefab entity with EntityInput"
            );
            anyhow::ensure!(
                sim.world.store.component_bytes(entity, ty)?.len() == 96,
                "invalid EntityInput layout"
            );
            println!("Experimental camera-relative WASD input for {entity:?}; original locomotion and gameplay animation systems are not implemented.");
            Some((entity, ty))
        } else {
            None
        };
        let mut loaded = load_scene_view(renderer, &content, &mut data)?;
        if opt.diagnostic_hud {
            loaded.hud = Some(DiagnosticHud::load(renderer, &content)?);
            println!("Diagnostic HUD: eight original action-bar frames; no inventory, controls, health or game HUD state.");
        }
        println!("{}", data.stats.report());
        let live = sim.map(|sim| (sim, data));
        return Ok((loaded, fly, live, control, animation, opt.follow_player));
    }
    let spec = scene::showcase(&content, opt.items);
    println!("scene: {} items", spec.len());
    Ok((
        load_scene(renderer, &content, &spec)?,
        None,
        None,
        None,
        None,
        false,
    ))
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
        place_terrain(opt, &content, &layouts, scene_path, &mut data)?;
        let cam = if opt.spawn_player {
            let mut sim = scene_view::simulate(&layouts, scene_path, 0)?;
            let entity = scene_view::spawn_player(&mut sim.world, &opt.data, &layouts)?;
            if let Some(position) = opt.player_position {
                scene_view::set_diagnostic_player_position(&mut sim.world, entity, position)?;
                println!("Diagnostic player position {position:?}: not an original spawn, collision unchecked.");
            }
            sim.advance(16_666_667, 1)?;
            let instance = sim
                .instance(entity)
                .context("spawned player's LocalToWorld unavailable")?;
            let first_group = data.groups.len();
            let parts = scene_view::attach_player_visual(&content, &mut data, entity, instance)?;
            if let Some(frame) = opt.animation_frame {
                let (_, samples) = scene_view::player_walk_samples(&content, 24)?;
                anyhow::ensure!(
                    samples.len() == parts,
                    "animation parts differ from visual parts"
                );
                for (group, frames) in data.groups[first_group..].iter_mut().zip(samples) {
                    group.verts = std::sync::Arc::new(
                        frames
                            .into_iter()
                            .nth(frame)
                            .context("missing animation frame")?,
                    );
                }
            }
            player_camera(data.bounds, instance)
        } else {
            scene_view::FlyCam::framing(data.bounds)
        };
        let mut loaded = load_scene_view(&renderer, &content, &mut data)?;
        if opt.diagnostic_hud {
            loaded.hud = Some(DiagnosticHud::load(&renderer, &content)?);
        }
        println!("{}", data.stats.report());
        let mut renderer = renderer;
        let uniforms = cam.uniforms(opt.width as f32 / opt.height as f32);
        let neutral = renderer.neutral_view();
        let draws = loaded.draws(&neutral);
        let rects = loaded
            .hud
            .as_ref()
            .map_or_else(Vec::new, |hud| hud.rects(opt.width, opt.height));
        let overlay: Vec<_> = loaded
            .hud
            .as_ref()
            .map(|hud| rrising_render::overlay::OverlayDraw {
                texture: &hud.texture.view,
                rects: &rects,
                uv: [0.0, 0.0, 1.0, 1.0],
            })
            .into_iter()
            .collect();
        let (w, h, rgba) = renderer.render_to_image_with_overlay(&uniforms, &draws, &overlay)?;
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
        diagnostic_overlay: None,
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
    let camera = if opt.spawn_player {
        let mut sim = scene_view::simulate(&layouts, scene_path, 0)?;
        let entity = scene_view::spawn_player(&mut sim.world, &opt.data, &layouts)?;
        if let Some(position) = opt.player_position {
            scene_view::set_diagnostic_player_position(&mut sim.world, entity, position)?;
        }
        sim.advance(scene_view::STEP_NS, 1)?;
        let instance = sim
            .instance(entity)
            .context("spawned player transform missing")?;
        let first = data.groups.len();
        let parts = scene_view::attach_player_visual(&content, &mut data, entity, instance)?;
        if let Some(frame) = opt.animation_frame {
            let (_, samples) = scene_view::player_walk_samples(&content, 24)?;
            anyhow::ensure!(
                samples.len() == parts,
                "animation parts differ from visual parts"
            );
            for (group, frames) in data.groups[first..].iter_mut().zip(samples) {
                group.verts = Arc::new(
                    frames
                        .into_iter()
                        .nth(frame)
                        .context("missing animation frame")?,
                );
            }
        }
        println!("Exporting static diagnostic player; no browser player simulation or controls.");
        player_camera(data.bounds, instance)
    } else {
        scene_view::FlyCam::framing(data.bounds)
    };
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
    let diagnostic_overlay = if opt.diagnostic_hud {
        let source = rrising_content::hud::candidate_action_bar(&content)?;
        let layout = action_bar_layout(&source);
        layout.validate()?;
        let texture =
            rrising_render::unity_texture::export(&content.read_texture(&source.texture)?)?;
        Some(rrising_render::snapshot::SnapshotOverlay { texture, layout })
    } else {
        None
    };
    let snapshot = Snapshot {
        version: 1,
        description: "Original scene entities at original LocalToWorld placements; not gameplay, no lighting parity".into(),
        uniforms: camera.uniforms(opt.width as f32 / opt.height as f32),
        items,
        textures: pool,
        diagnostic_overlay,
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

/// List eligible input components; this does not identify which entity is a player.
fn list_input_entities(opt: &Options, path: &Path) -> Result<()> {
    let layout_path = opt
        .type_layouts
        .as_ref()
        .context("--list-input-entities needs --type-layouts")?;
    let layouts =
        rrising_content::type_layouts::TypeLayouts::from_json(&std::fs::read(layout_path)?)?;
    let scene = rrising_content::entity_scenes::SceneFile::open(path)?;
    let sw = rrising_content::world_adapter::build_scene_world(&layouts, &scene)?;
    let ty = sw.world.types.type_index_of("ProjectM.EntityInput");
    let entities = match ty {
        Some(ty) => sw.world.store.query_entities(&rrising_ecs::EntityQuery {
            all: vec![ty],
            ..Default::default()
        })?,
        None => vec![],
    };
    let rows: Vec<_> = entities
        .into_iter()
        .map(|e| serde_json::json!({"index":e.index,"version":e.version}))
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"scene":path.display().to_string(), "scope":"live non-prefab entities with EntityInput; not player identification", "entities":rows})
        )?
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
    if let Some(guid) = opt.clone_prefab {
        use rrising_content::{
            prefab_catalog::ScenePrefabCatalog,
            type_layouts::{TypeLayouts, ENTITY_HASH},
        };
        use rrising_ecs::{EntityComponentStore, SpecialTypes, TypeManager, TypeSpec};
        let path = opt
            .type_layouts
            .as_ref()
            .context("missing server layout report")?;
        let layouts = TypeLayouts::from_server_json(&std::fs::read(path)?)?;
        let entity = layouts.get(ENTITY_HASH).context("missing entity layout")?;
        anyhow::ensure!(
            entity.portable_value,
            "regenerate the extended server layout report"
        );
        let template = ScenePrefabCatalog::read_server_linked_prefab(&opt.data, &layouts, guid)?;
        let mut types = TypeManager::default();
        types.register(TypeSpec::tag("<reserved null>"))?;
        let entity_type = types.register(entity.spec.clone())?;
        let mut store = EntityComponentStore::new(
            i32::try_from(template.members.len() + 2)?,
            SpecialTypes::new(entity_type),
        )?;
        let handles = template.instantiate(&mut store, &mut types, &layouts)?;
        let copied_values: usize = template.members.iter().map(Vec::len).sum();
        let references: usize = template
            .members
            .iter()
            .flatten()
            .map(|value| value.reference_patches.len())
            .sum();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "kind": "server-prefab-clone-projection", "prefab_guid": guid,
                "entities": handles.len(), "copied_values": copied_values,
                "remapped_references": references,
            }))?
        );
        return Ok(());
    }
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
            let mega_statics =
                rrising_content::persistence::MegaStaticRestorePlan::read(&snapshot)?;
            let requested = mega_statics
                .entries
                .iter()
                .map(|entry| entry.prefab_guid)
                .collect();
            let catalogs =
                ScenePrefabCatalog::read_server_game_data_for(&opt.data, &layouts, &requested)?;
            let saved_blobs =
                ScenePrefabCatalog::read_saved_prefab_blobs(&opt.data, &layouts, header)?;
            let root_blobs = if [
                0x9534_7421_34ef_4051,
                0x9f76_a48d_02bc_e2bb,
                0xa413_a8bb_b072_50bc,
                0x779b_44a3_f06b_eeb7,
            ]
            .iter()
            .all(|&hash| {
                layouts
                    .get(hash)
                    .is_some_and(|layout| layout.blob_offsets.is_some())
            }) {
                Some(ScenePrefabCatalog::read_saved_root_blobs(
                    &opt.data, &layouts, &snapshot,
                )?)
            } else {
                None
            };
            let terrain = rrising_content::static_transforms::StaticTransformCatalog::read_server(
                &opt.data, &layouts,
            )?;
            let restored = if let Some(blobs) = root_blobs {
                RestoredWorldProjection::build_with_terrain_and_all_blobs(
                    &snapshot,
                    &layouts,
                    &catalogs,
                    &mega_statics,
                    &terrain,
                    blobs,
                    &saved_blobs,
                )?
            } else {
                RestoredWorldProjection::build_with_terrain(
                    &snapshot,
                    &layouts,
                    &catalogs,
                    &mega_statics,
                    &terrain,
                )?
            };
            let baked_matches = mega_statics
                .entries
                .iter()
                .filter(|entry| {
                    terrain
                        .get(entry.terrain_chunk, entry.transform_index)
                        .is_some()
                })
                .count();
            Some(serde_json::json!({
                "live_entities": restored.bindings.len() - 1,
                "prefab_roots": restored.prefabs.len(),
                "prefab_blob_sources": {
                    "matched_fields": saved_blobs.matched_fields,
                    "nonnull_fields": saved_blobs.nonnull_fields,
                    "missing_fields": saved_blobs.missing_fields,
                    "unbound_fields": saved_blobs.unbound_fields,
                    "payloads": saved_blobs.payloads.len(),
                    "conflicts": saved_blobs.conflicts,
                    "null_mismatches": saved_blobs.null_mismatches,
                },
                "prefab_root_blob_components": restored.prefab_root_blobs().map(|blobs| blobs.values.len()),
                "prefab_blob_bindings": restored.prefab_blob_bindings.len(),
                "scalar_values": restored.copied_scalar_values,
                "buffer_rows": restored.copied_buffer_rows,
                "buffer_elements": restored.copied_buffer_elements,
                "prefab_linked_groups": restored.prefab_linked_groups,
                "admitted_saved_types": restored.admitted_saved_types,
                "admitted_saved_tag_types": restored.admitted_saved_tag_types,
                "skipped_saved_tag_types": restored.skipped_saved_tag_types,
                "initialized_simulate_rows": restored.initialized_simulate_rows,
                "restored_disabled_rows": restored.restored_disabled_rows,
                "skipped_saved_types": restored.skipped_saved_types,
                "skipped_saved_components": restored.skipped_saved_components,
                "cleared_references": restored.cleared_references,
                "mega_static_plan": {
                    "managers": mega_statics.manager_rows,
                    "instances": mega_statics.entries.len(),
                    "placed_instances": restored.placed_mega_statics,
                    "template_roots": restored.mega_static_template_roots,
                    "template_values_copied": restored.mega_static_template_values,
                    "template_self_references": restored.mega_static_template_self_references,
                    "vacant_slots": mega_statics.vacant_slots,
                    "prefab_guids": mega_statics.distinct_prefab_guids,
                    "baked_transform_matches": baked_matches,
                    "terrain_chunks": terrain.chunk_count(),
                    "terrain_templates": terrain.template_count(),
                },
                "missing_references": {
                    "nonpersistent": restored.missing_references.nonpersistent,
                    "stale_version": restored.missing_references.stale_version,
                    "outside_world_table": restored.missing_references.outside_world_table,
                    "unlisted": restored.missing_references.unlisted,
                    "distinct_handles": restored.missing_references.distinct_handles,
                    "by_component": restored.missing_references.by_component,
                },
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
    if let Some(path) = &opt.list_input_entities {
        return list_input_entities(&opt, path);
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
    use super::{diff_images, follow_camera};

    #[test]
    fn diagnostic_animation_tracks_horizontal_displacement_and_stops() {
        let mut motion = super::MotionAnimation {
            previous: [1.0, 2.0, 3.0],
            phase_seconds: 0.0,
            moving: false,
        };
        assert_eq!(motion.frame(1.0, 4), None);
        motion.advance([1.0, 3.0, 3.0], 0.25, 1.0);
        assert_eq!(motion.frame(1.0, 4), None);
        motion.advance([2.0, 3.0, 3.0], 0.25, 1.0);
        assert_eq!(motion.frame(1.0, 4), Some(1));
        motion.advance([2.0, 3.0, 3.0], 0.5, 1.0);
        assert_eq!(motion.frame(1.0, 4), None);
        motion.advance([2.0, 3.0, 4.0], 0.25, 1.0);
        assert_eq!(motion.frame(1.0, 4), Some(2));
        motion.advance([3.0, 3.0, 4.0], 0.5, 1.0);
        assert_eq!(motion.frame(1.0, 4), Some(0));
    }

    #[test]
    fn diagnostic_position_requires_three_finite_values() {
        assert_eq!(
            super::parse_player_position("1,-2.5,3").unwrap(),
            [1.0, -2.5, 3.0]
        );
        for invalid in ["1,2", "1,2,3,4", "nan,0,0", "inf,1,2", "bad,0,0"] {
            assert!(super::parse_player_position(invalid).is_err());
        }
    }

    #[test]
    fn camera_follows_displacement_without_jumping_on_first_observation() {
        let mut camera = super::scene_view::FlyCam::framing(([0.; 3], [1.; 3]));
        camera.eye = [1., 3., -5.];
        let mut previous = None;
        follow_camera(&mut camera, &mut previous, [10., 2., 3.]);
        assert_eq!(camera.eye, [1., 3., -5.]);
        follow_camera(&mut camera, &mut previous, [12., 3., 2.]);
        assert_eq!(camera.eye, [3., 4., -6.]);
        follow_camera(&mut camera, &mut previous, [f32::NAN, 0., 0.]);
        assert_eq!(previous, Some([12., 3., 2.]));
    }

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
