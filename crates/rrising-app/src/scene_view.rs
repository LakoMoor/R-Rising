//! `--scene <file.entities>`: draw an original scene's entities at their original placements.
//!
//! The chain, every link read from the owner's install and none of it simulated:
//! 1. the scene's chunks are resolved through the recovered type layouts, and each entity's
//!    `LocalToWorld` (a serialized 4x4 matrix) is read from its chunk bytes;
//! 2. `MaterialMeshInfo` gives static indices into the scene's `ManagedRenderMeshArray` (the first
//!    managed shared component), whose two int arrays index the scene's `ReferencedUnityObjects`
//!    asset, a MonoBehaviour in the content archives holding one `PPtr` per referenced object;
//! 3. the `ReferencedUnityObjects` asset is found through the runtime catalog: the scene's
//!    `.entityheader` embeds the scene GUID that the catalog's object records carry.
//!
//! Anything that does not resolve (runtime ids, index ranges, unreadable or skinned meshes, a
//! scene whose managed array cannot be located) is drawn as a marker box and counted. Sub-mesh
//! selection and per-material shader state are not applied: a mesh is drawn whole with the
//! material's base map.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, ensure, Context, Result};
use rrising_content::catalog::RuntimeContentCatalog;
use rrising_content::entity_scenes::SceneFile;
use rrising_content::type_layouts::{TypeLayouts, LOCAL_TO_WORLD_HASH};
use rrising_content::GameContent;
use rrising_ecs::MaterialMeshInfo;
use rrising_math::{float3, float4x4};
use rrising_render::{mesh, FrameUniforms, Instance, Vertex};
use unity_formats::player::{ObjRef, PPtr};

type CpuMesh = (usize, Arc<Vec<Vertex>>, Arc<Vec<u32>>, String);
type RefTable = (ObjRef, Vec<PPtr>, Vec<i32>, Vec<i32>);

const MATERIAL_MESH_INFO_HASH: u64 = 0x3ae3_a524_8a68_9306;
const MANAGED_RENDER_MESH_ARRAY_HASH: u64 = 0xf423_fa42_b77d_7330;

/// What the scene pass measured. Printed verbatim by the app.
#[derive(Default, Debug, Clone)]
pub struct SceneStats {
    pub entities: usize,
    pub with_transform: usize,
    pub with_material_mesh_info: usize,
    pub unique_meshes_resolved: usize,
    pub unique_textures: usize,
    pub resolved_entities: usize,
    pub drawn_mesh_instances: usize,
    pub marker_instances: usize,
    /// Drawn instances whose material had no resolvable base map (drawn with the neutral white texture).
    pub untextured_instances: usize,
    pub unresolved_runtime_or_range: usize,
    pub unresolved_index: usize,
    pub unresolved_mesh_read: usize,
    pub referenced_objects: usize,
    pub note: String,
}

/// One instanced draw of a resolved mesh.
pub struct ReadyGroup {
    pub name: String,
    pub mesh_id: usize,
    pub verts: Arc<Vec<Vertex>>,
    pub inds: Arc<Vec<u32>>,
    pub texture: Option<ObjRef>,
    pub instances: Vec<Instance>,
}

pub struct SceneData {
    pub groups: Vec<ReadyGroup>,
    pub markers: Vec<Instance>,
    pub stats: SceneStats,
    /// Percentile-trimmed world bounds of everything drawn.
    pub bounds: ([f32; 3], [f32; 3]),
}

struct RawGroup {
    mesh: ObjRef,
    texture: Option<ObjRef>,
    instances: Vec<Instance>,
}

fn guid_windows_to_file_name(value: [u32; 4]) -> String {
    // Unity prints a GUID as nibble-swapped bytes of the four little-endian words.
    value
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .map(|b| format!("{:02x}", b.rotate_left(4)))
        .collect()
}

/// Locate the scene's `ReferencedUnityObjects` MonoBehaviour through the catalog.
fn find_referenced_objects(
    content: &GameContent,
    catalog: &RuntimeContentCatalog,
    scene_path: &Path,
) -> Result<ObjRef> {
    let name = scene_path
        .file_name()
        .and_then(|n| n.to_str())
        .context("scene name")?;
    let stem = name
        .split('.')
        .next()
        .context("scene name has no stem")?
        .to_owned();
    let header_path = scene_path.with_file_name(format!("{stem}.entityheader"));
    let header = std::fs::read(&header_path)
        .with_context(|| format!("reading {}", header_path.display()))?;
    let mut by_guid: HashMap<[u8; 16], Vec<usize>> = HashMap::new();
    for (i, o) in catalog.objects.iter().enumerate() {
        let mut g = [0u8; 16];
        g.copy_from_slice(&o.object_id[16..32]);
        if g != [0; 16] {
            by_guid.entry(g).or_default().push(i);
        }
    }
    for w in header.windows(16) {
        let Some(hits) = w.try_into().ok().and_then(|k: [u8; 16]| by_guid.get(&k)) else {
            continue;
        };
        for &i in hits {
            let o = &catalog.objects[i];
            let file = guid_windows_to_file_name(catalog.files[o.file_index].id.value);
            let r = ObjRef {
                file,
                path_id: o.local_identifier,
            };
            if content.player().class_id(&r).is_some()
                && content
                    .player()
                    .script_class(&r)
                    .ok()
                    .flatten()
                    .as_deref()
                    .is_some_and(|c| c == "Unity.Scenes.ReferencedUnityObjects")
            {
                return Ok(r);
            }
        }
    }
    bail!("no ReferencedUnityObjects asset found for this scene in the catalog")
}

fn parse_pptr_table(bytes: &[u8]) -> Result<Vec<PPtr>> {
    ensure!(
        bytes.len() >= 4,
        "ReferencedUnityObjects script data is empty"
    );
    let n = u32::from_le_bytes(bytes[0..4].try_into()?) as usize;
    ensure!(
        n.checked_mul(12).and_then(|x| x.checked_add(4)) <= Some(bytes.len()),
        "PPtr table overruns the asset"
    );
    Ok((0..n)
        .map(|i| {
            let b = &bytes[4 + i * 12..16 + i * 12];
            PPtr {
                file_id: i32::from_le_bytes(b[0..4].try_into().unwrap()),
                path_id: i64::from_le_bytes(b[4..12].try_into().unwrap()),
            }
        })
        .collect())
}

/// `[2 header bytes][i32 n][n x i32][1 byte][i32 m][m x i32]`: two arrays of indices into the
/// referenced-objects table (materials and meshes). Layout observed on the install and validated
/// by requiring every index to be in range and each array to resolve to a single class.
fn parse_index_arrays(blob: &[u8], limit: usize) -> Result<(Vec<i32>, Vec<i32>)> {
    fn read(blob: &[u8], at: &mut usize, limit: usize) -> Result<Vec<i32>> {
        let n = i32::from_le_bytes(blob.get(*at..*at + 4).context("short blob")?.try_into()?);
        ensure!(n >= 0, "negative array length");
        let n = n as usize;
        *at += 4;
        let raw = blob.get(*at..*at + n * 4).context("array overruns blob")?;
        *at += n * 4;
        let v: Vec<i32> = raw
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| i32::from_le_bytes(*c))
            .collect();
        ensure!(
            v.iter().all(|&x| x >= 0 && (x as usize) < limit),
            "index outside the referenced-objects table"
        );
        Ok(v)
    }
    let mut at = 2;
    let a = read(blob, &mut at, limit)?;
    at += 1;
    let b = read(blob, &mut at, limit)?;
    Ok((a, b))
}

fn matrix_instance(m: [f32; 16]) -> Option<Instance> {
    if m.iter().any(|x| !x.is_finite()) {
        return None;
    }
    Some(Instance {
        model: [
            [m[0], m[1], m[2], m[3]],
            [m[4], m[5], m[6], m[7]],
            [m[8], m[9], m[10], m[11]],
            [m[12], m[13], m[14], m[15]],
        ],
    })
}

/// Collect placements and resolve references. Meshes are read once per unique object.
pub fn collect(
    content: &GameContent,
    data_dir: &Path,
    layouts_path: &Path,
    scene_path: &Path,
) -> Result<SceneData> {
    let layouts = TypeLayouts::from_json(&std::fs::read(layouts_path)?)?;
    let scene = SceneFile::open(scene_path)?;
    let resolved = layouts.resolve_scene(&scene)?;
    let mut stats = SceneStats::default();

    // Reference tables. Failure here leaves every renderer as a marker, with the reason recorded.
    let table = (|| -> Result<RefTable> {
        let catalog = RuntimeContentCatalog::open(
            &data_dir.join("StreamingAssets/ContentArchives/archive_dependencies.bin"),
        )?;
        let ruo = find_referenced_objects(content, &catalog, scene_path)?;
        let (_, bytes) = content
            .player()
            .behaviour(&ruo)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let pptrs = parse_pptr_table(&bytes)?;
        let sc = scene.shared_and_managed_components()?;
        let mrma = sc
            .managed_shared_records
            .iter()
            .filter(|r| r.stable_type_hash == MANAGED_RENDER_MESH_ARRAY_HASH)
            .count();
        ensure!(
            sc.managed_shared_records
                .first()
                .is_some_and(|r| r.stable_type_hash == MANAGED_RENDER_MESH_ARRAY_HASH),
            "first managed shared component is not a ManagedRenderMeshArray"
        );
        ensure!(
            mrma == 1,
            "{mrma} ManagedRenderMeshArray components; the chunk-to-array mapping is not recovered"
        );
        let (a, b) = parse_index_arrays(sc.managed_blob, pptrs.len())?;
        Ok((ruo, pptrs, a, b))
    })();

    let mut meshes_arr: Vec<ObjRef> = Vec::new();
    let mut materials_arr: Vec<Option<ObjRef>> = Vec::new();
    match table {
        Ok((ruo, pptrs, a, b)) => {
            stats.referenced_objects = pptrs.len();
            let resolve = |i: i32| -> Option<ObjRef> {
                content
                    .player()
                    .resolve_pptr(&ruo.file, pptrs[i as usize])
                    .ok()
                    .flatten()
            };
            let class_of = |v: &[i32]| {
                let mut counts: HashMap<&str, usize> = HashMap::new();
                for &i in v {
                    if let Some(r) = resolve(i) {
                        if let Some(c) = content.player().class_name(&r) {
                            *counts.entry(c).or_default() += 1;
                        }
                    }
                }
                counts
                    .into_iter()
                    .max_by_key(|(_, n)| *n)
                    .map(|(c, _)| c.to_owned())
            };
            let (ca, cb) = (class_of(&a), class_of(&b));
            let (mesh_idx, mat_idx) = match (ca.as_deref(), cb.as_deref()) {
                (Some("Mesh"), Some("Material")) => (a, b),
                (Some("Material"), Some("Mesh")) => (b, a),
                other => {
                    stats.note = format!("array classes {other:?} are not Mesh/Material");
                    (vec![], vec![])
                }
            };
            for i in mesh_idx {
                meshes_arr.push(resolve(i).unwrap_or(ObjRef {
                    file: String::new(),
                    path_id: 0,
                }));
            }
            for i in mat_idx {
                materials_arr.push(resolve(i));
            }
        }
        Err(e) => stats.note = format!("{e:#}"),
    }

    // Texture per material, resolved once.
    let mut tex_of_material: HashMap<ObjRef, Option<ObjRef>> = HashMap::new();
    let mut raw: HashMap<(ObjRef, Option<ObjRef>), RawGroup> = HashMap::new();
    let mut markers: Vec<Instance> = Vec::new();

    for (ci, chunk) in resolved.chunks.iter().enumerate() {
        for e in 0..chunk.header.count as usize {
            stats.entities += 1;
            let Some(ltw) = resolved.component_f32s::<16>(ci, e, LOCAL_TO_WORLD_HASH)? else {
                continue;
            };
            let Some(inst) = matrix_instance(ltw) else {
                continue;
            };
            stats.with_transform += 1;
            let Some(bytes) = resolved.component_bytes(ci, e, MATERIAL_MESH_INFO_HASH)? else {
                continue;
            };
            stats.with_material_mesh_info += 1;
            let info = MaterialMeshInfo::from_bytes(bytes)?;
            if info.has_material_mesh_index_range() || info.is_runtime_mesh() {
                stats.unresolved_runtime_or_range += 1;
                markers.push(inst);
                continue;
            }
            let mi = info.mesh_array_index();
            let mat = info.material_array_index();
            let mesh_ref = usize::try_from(mi)
                .ok()
                .and_then(|i| meshes_arr.get(i))
                .filter(|r| r.path_id != 0);
            let Some(mesh_ref) = mesh_ref else {
                stats.unresolved_index += 1;
                markers.push(inst);
                continue;
            };
            let material = usize::try_from(mat)
                .ok()
                .and_then(|i| materials_arr.get(i))
                .cloned()
                .flatten();
            let texture = material.and_then(|m| {
                tex_of_material
                    .entry(m.clone())
                    .or_insert_with(|| {
                        let mat = content.read_material(&m).ok()?;
                        let slot = rrising_core::base_map_name(&mat)?;
                        mat.textures
                            .iter()
                            .find(|(n, _)| n == slot)
                            .and_then(|(_, env)| env.texture.clone())
                    })
                    .clone()
            });
            raw.entry((mesh_ref.clone(), texture.clone()))
                .or_insert_with(|| RawGroup {
                    mesh: mesh_ref.clone(),
                    texture,
                    instances: vec![],
                })
                .instances
                .push(inst);
        }
    }

    // Read each unique mesh once; a group whose mesh cannot be read becomes markers.
    let mut cpu: HashMap<ObjRef, Option<CpuMesh>> = HashMap::new();
    let mut groups = Vec::new();
    let mut keys: Vec<_> = raw.into_iter().collect();
    keys.sort_by(|a, b| (&a.0 .0, a.0 .1.as_ref()).cmp(&(&b.0 .0, b.0 .1.as_ref())));
    for (_, g) in keys {
        let next_id = cpu.len();
        let entry = cpu.entry(g.mesh.clone()).or_insert_with(|| {
            let m = content.read_mesh(&g.mesh).ok()?;
            if m.skin.is_some() {
                return None;
            }
            let (v, i) = mesh::build_vertices(&m).ok()?;
            if v.is_empty() || i.is_empty() {
                return None;
            }
            Some((next_id, Arc::new(v), Arc::new(i), m.name.clone()))
        });
        match entry {
            Some((id, v, i, name)) => groups.push(ReadyGroup {
                name: name.clone(),
                mesh_id: *id,
                verts: v.clone(),
                inds: i.clone(),
                texture: g.texture,
                instances: g.instances,
            }),
            None => {
                stats.unresolved_mesh_read += g.instances.len();
                markers.extend(g.instances);
            }
        }
    }
    stats.unique_meshes_resolved = cpu.values().filter(|v| v.is_some()).count();
    stats.unique_textures = groups
        .iter()
        .filter_map(|g| g.texture.as_ref())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    stats.drawn_mesh_instances = groups.iter().map(|g| g.instances.len()).sum();
    stats.resolved_entities = stats.drawn_mesh_instances;
    stats.marker_instances = markers.len();
    stats.untextured_instances = groups
        .iter()
        .filter(|g| g.texture.is_none())
        .map(|g| g.instances.len())
        .sum();

    // Trimmed bounds: 1st..99th percentile of translations, so a few far outliers do not shrink the
    // framed scene to a dot.
    let mut axes: [Vec<f32>; 3] = Default::default();
    for inst in groups.iter().flat_map(|g| &g.instances).chain(&markers) {
        for (a, axis) in axes.iter_mut().enumerate() {
            axis.push(inst.model[3][a]);
        }
    }
    let mut lo = [0.0; 3];
    let mut hi = [0.0; 3];
    for a in 0..3 {
        let v = &mut axes[a];
        if v.is_empty() {
            continue;
        }
        v.sort_by(|x, y| x.total_cmp(y));
        lo[a] = v[v.len() / 100];
        hi[a] = v[v.len() - 1 - v.len() / 100];
    }
    Ok(SceneData {
        groups,
        markers,
        stats,
        bounds: (lo, hi),
    })
}

/// A unit cube centred on the origin, for entities whose mesh did not resolve.
pub fn marker_box() -> (Vec<Vertex>, Vec<u32>) {
    let mut v = Vec::new();
    let mut i = Vec::new();
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1., 0., 0.], [0., 1., 0.], [0., 0., 1.]),
        ([-1., 0., 0.], [0., 1., 0.], [0., 0., -1.]),
        ([0., 1., 0.], [0., 0., 1.], [1., 0., 0.]),
        ([0., -1., 0.], [0., 0., -1.], [1., 0., 0.]),
        ([0., 0., 1.], [0., 1., 0.], [-1., 0., 0.]),
        ([0., 0., -1.], [0., 1., 0.], [1., 0., 0.]),
    ];
    for (n, up, right) in faces {
        let base = v.len() as u32;
        for (u, w, uv) in [
            (-1., -1., [0., 0.]),
            (1., -1., [1., 0.]),
            (1., 1., [1., 1.]),
            (-1., 1., [0., 1.]),
        ] {
            let p = [0, 1, 2].map(|k| 0.5 * (n[k] + right[k] * u + up[k] * w));
            v.push(Vertex {
                position: p,
                normal: n,
                uv,
            });
        }
        i.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (v, i)
}

/// Free-flying camera in Unity's left-handed, +Y-up world.
#[derive(Clone, Copy)]
pub struct FlyCam {
    pub eye: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
    pub near: f32,
    pub far: f32,
}

impl FlyCam {
    /// Frame the trimmed bounds from the south, looking down at a fixed pitch.
    pub fn framing(bounds: ([f32; 3], [f32; 3])) -> Self {
        let (lo, hi) = bounds;
        let c = [0, 1, 2].map(|a| 0.5 * (lo[a] + hi[a]));
        let radius = (0..3).map(|a| 0.5 * (hi[a] - lo[a])).fold(1.0f32, f32::max);
        let pitch = -0.75f32;
        let fov = std::f32::consts::FRAC_PI_3;
        let dist = radius / (fov * 0.5).tan() * 1.5;
        let fwd = [0.0, pitch.sin(), pitch.cos()];
        Self {
            eye: [
                c[0] - fwd[0] * dist,
                c[1] - fwd[1] * dist,
                c[2] - fwd[2] * dist,
            ],
            yaw: 0.0,
            pitch,
            speed: radius * 0.4,
            near: (dist * 0.01).max(0.3),
            far: dist * 4.0 + radius * 4.0,
        }
    }

    pub fn forward(&self) -> [f32; 3] {
        [
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        ]
    }

    /// Move along the view (`f`), sideways (`r`) and world up (`u`) for `dt` seconds.
    pub fn fly(&mut self, f: f32, r: f32, u: f32, dt: f32) {
        let fw = self.forward();
        let right = [self.yaw.cos(), 0.0, -self.yaw.sin()];
        for a in 0..3 {
            let up = if a == 1 { u } else { 0.0 };
            self.eye[a] += (fw[a] * f + right[a] * r + up) * self.speed * dt;
        }
    }

    pub fn uniforms(&self, aspect: f32) -> FrameUniforms {
        let eye = float3::new(self.eye[0], self.eye[1], self.eye[2]);
        let fw = self.forward();
        let target = float3::new(eye.x + fw[0], eye.y + fw[1], eye.z + fw[2]);
        // Same conventions as the showcase camera: view = flip_z * inverse(look_at).
        let camera_to_world = float4x4::look_at(eye, target, float3::new(0.0, 1.0, 0.0));
        let view = float4x4::scale(float3::new(1.0, 1.0, -1.0)) * camera_to_world.inverse();
        let proj =
            float4x4::perspective_fov(std::f32::consts::FRAC_PI_3, aspect, self.near, self.far);
        let view_proj = rrising_render::clip_fix() * proj * view;
        FrameUniforms {
            view_proj: Instance::from_matrix(&view_proj).model,
            camera_pos: [eye.x, eye.y, eye.z, 0.0],
            light_dir: [0.35, 0.85, 0.4, 0.85],
            ambient: [0.32, 0.36, 0.44, 0.0],
        }
    }
}

impl SceneStats {
    pub fn report(&self) -> String {
        format!(
            "scene: {} entities, {} with LocalToWorld, {} with MaterialMeshInfo | referenced objects {} | \
             resolved meshes {} unique ({} textures) | drawn mesh instances {} (without base map {}) | unresolved -> marker boxes {} \
             (runtime/range {}, bad index {}, mesh unreadable/skinned {}){}",
            self.entities,
            self.with_transform,
            self.with_material_mesh_info,
            self.referenced_objects,
            self.unique_meshes_resolved,
            self.unique_textures,
            self.drawn_mesh_instances,
            self.untextured_instances,
            self.marker_instances,
            self.unresolved_runtime_or_range,
            self.unresolved_index,
            self.unresolved_mesh_read,
            if self.note.is_empty() {
                String::new()
            } else {
                format!(" | note: {}", self.note)
            }
        )
    }
}

/// Convenience for callers: the layouts path from the option or `RRISING_LAYOUT_REPORT`.
pub fn layouts_path(opt: Option<&PathBuf>) -> Result<PathBuf> {
    opt.cloned()
        .or_else(|| std::env::var_os("RRISING_LAYOUT_REPORT").map(PathBuf::from))
        .context("--scene needs --type-layouts or RRISING_LAYOUT_REPORT")
}
