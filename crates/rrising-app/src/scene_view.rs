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
use rrising_ecs::{Entity, MaterialMeshInfo};
use rrising_math::{float3, float4x4};
use rrising_render::{mesh, FrameUniforms, Instance, Vertex};
use unity_formats::player::{ObjRef, PPtr};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Surface {
    Opaque,
    /// Alpha-tested material (`_ALPHATEST_ON`): drawn through the renderer's cutout pipeline.
    Cutout,
    /// Blended material (queue >= 3000 or a transparent surface flag): excluded, not drawn opaque.
    Transparent,
}

/// A material's draw-relevant facts, read once.
#[derive(Clone, Debug)]
struct MatInfo {
    texture: Option<ObjRef>,
    surface: Surface,
}

struct ArrayTable {
    meshes: Vec<ObjRef>,
    materials: Vec<Option<ObjRef>>,
}

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
    pub unresolved_no_array: usize,
    /// Number of `ManagedRenderMeshArray` tables located in the scene.
    pub render_mesh_arrays: usize,
    /// Entities left out because their material is alpha-blended.
    pub excluded_transparent: usize,
    /// Drawn instances whose material is alpha-tested (fixed 0.5 threshold, `_AlphaCutoff` not read).
    pub cutout_instances: usize,
    /// Entities left out because their sub-mesh is not a usable triangle list.
    pub excluded_submesh: usize,
    /// Drawn instances that use one sub-mesh of a multi-sub-mesh mesh.
    pub submesh_instances: usize,
    /// Instances whose base map failed to upload and were drawn with the grey fallback (set by the app).
    pub texture_fallback_instances: usize,
    pub texture_fallback_textures: usize,
    /// Drawn instances whose base map is a `Texture2DArray` shown as layer 0 (layer selection not recovered).
    pub array_layer0_instances: usize,
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
    /// Alpha-tested material: drawn through the cutout pipeline.
    pub cutout: bool,
    pub instances: Vec<Instance>,
    /// Stored entity id of each instance (same order), to find it again in the simulated world.
    pub entities: Vec<Entity>,
}

pub struct SceneData {
    pub groups: Vec<ReadyGroup>,
    pub markers: Vec<Instance>,
    pub marker_entities: Vec<Entity>,
    pub stats: SceneStats,
    /// Percentile-trimmed world bounds of everything drawn.
    pub bounds: ([f32; 3], [f32; 3]),
}

/// Merge placed static sections; mesh ids are local to each section and must be remapped.
pub fn append_static_section(data: &mut SceneData, mut other: SceneData) {
    let offset = data
        .groups
        .iter()
        .map(|g| g.mesh_id)
        .max()
        .map_or(0, |n| n + 1);
    for group in &mut other.groups {
        group.mesh_id += offset;
    }
    for axis in 0..3 {
        data.bounds.0[axis] = data.bounds.0[axis].min(other.bounds.0[axis]);
        data.bounds.1[axis] = data.bounds.1[axis].max(other.bounds.1[axis]);
    }
    data.groups.append(&mut other.groups);
    data.markers.append(&mut other.markers);
    data.marker_entities.append(&mut other.marker_entities);
    data.stats.note = "Statistics describe the first section only".into();
}

/// Apply the recovered section placement to the baked draw matrices. This does
/// not reproduce terrain streaming or prove correspondence to a live world.
pub fn place_terrain_section(data: &mut SceneData, placement: float4x4) {
    use rrising_math::float4;
    let place = |instance: &mut Instance| {
        let c = instance.model.map(|v| float4::new(v[0], v[1], v[2], v[3]));
        let local = float4x4::new(c[0], c[1], c[2], c[3]);
        *instance = Instance::from_matrix(&float4x4::mul_matrix(placement, local));
    };
    for instance in data
        .groups
        .iter_mut()
        .flat_map(|g| &mut g.instances)
        .chain(&mut data.markers)
    {
        place(instance);
    }
    let mut axes: [Vec<f32>; 3] = Default::default();
    for instance in data
        .groups
        .iter()
        .flat_map(|g| &g.instances)
        .chain(&data.markers)
    {
        for (axis, values) in axes.iter_mut().enumerate() {
            values.push(instance.model[3][axis]);
        }
    }
    let (mut lo, mut hi) = ([0.; 3], [0.; 3]);
    for (axis, values) in axes.iter_mut().enumerate() {
        if !values.is_empty() {
            values.sort_by(f32::total_cmp);
            lo[axis] = values[values.len() / 100];
            hi[axis] = values[values.len() - 1 - values.len() / 100];
        }
    }
    data.bounds = (lo, hi);
}

struct RawGroup {
    cutout: bool,
    texture: Option<ObjRef>,
    instances: Vec<Instance>,
    entities: Vec<Entity>,
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
/// by requiring every index to be in range and each array to resolve to a single class. Returns the
/// arrays and the offset just past the second one.
fn parse_index_arrays(blob: &[u8], limit: usize) -> Result<(Vec<i32>, Vec<i32>, usize)> {
    fn read(blob: &[u8], at: &mut usize, limit: usize) -> Result<Vec<i32>> {
        let n = i32::from_le_bytes(blob.get(*at..*at + 4).context("short blob")?.try_into()?);
        ensure!(n >= 0, "negative array length");
        let n = n as usize;
        *at += 4;
        let raw = blob
            .get(*at..n.checked_mul(4).context("overflow")? + *at)
            .context("array overruns blob")?;
        *at += n * 4;
        let v: Vec<i32> = raw
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| i32::from_le_bytes(*c))
            .collect();
        ensure!(
            v.iter()
                .all(|&x| x == -1 || (x >= 0 && (x as usize) < limit)),
            "index outside the referenced-objects table"
        );
        Ok(v)
    }
    let mut at = 2;
    let a = read(blob, &mut at, limit)?;
    at += 1;
    let b = read(blob, &mut at, limit)?;
    Ok((a, b, at))
}

/// Only static mesh and material array indices can be resolved from baked scene assets.
fn needs_runtime_or_range_resolution(info: MaterialMeshInfo) -> bool {
    info.has_material_mesh_index_range() || info.is_runtime_mesh() || info.is_runtime_material()
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

/// Classify a material from facts stored in the file only: HDRP's `_SURFACE_TYPE_TRANSPARENT`
/// keyword / `_SurfaceType` float, an explicit custom render queue, and the alpha-test flags. The
/// shader's own default queue is not read, so a transparent material that relies on it alone is
/// still drawn as opaque; that limit is reported, not hidden.
fn classify_material(m: &unity_formats::material::UnityMaterialData) -> (Surface, String) {
    let kw = |k: &str| m.keywords.iter().any(|x| x == k);
    let f = |n: &str| m.floats.iter().find(|(k, _)| k == n).map(|(_, v)| *v);
    let transparent =
        kw("_SURFACE_TYPE_TRANSPARENT") || f("_SurfaceType") == Some(1.0) || m.render_queue >= 3000;
    let cutout = kw("_ALPHATEST_ON")
        || f("_AlphaCutoffEnable") == Some(1.0)
        || f("_AlphaClip") == Some(1.0)
        || (2450..3000).contains(&m.render_queue);
    let surface = if transparent {
        Surface::Transparent
    } else if cutout {
        Surface::Cutout
    } else {
        Surface::Opaque
    };
    let summary = format!(
        "{:?} queue {} surfaceType {:?} alphaCutoffEnable {:?} kw {:?} slots {:?}",
        m.name,
        m.render_queue,
        f("_SurfaceType"),
        f("_AlphaCutoffEnable"),
        m.keywords,
        m.textures
            .iter()
            .filter(|(_, e)| e.texture.is_some())
            .map(|(n, _)| n.as_str())
            .collect::<Vec<_>>()
    );
    (surface, summary)
}

type SubMeshGpu = (usize, Arc<Vec<Vertex>>, Arc<Vec<u32>>);

/// Keep only the vertices a sub-mesh's indices reach, so each sub-mesh draw carries its own
/// vertices and not a copy of the whole mesh.
fn compact_vertices(verts: &[Vertex], inds: &[u32]) -> (Arc<Vec<Vertex>>, Vec<u32>) {
    let mut remap: HashMap<u32, u32> = HashMap::new();
    let mut out = Vec::new();
    let new_inds = inds
        .iter()
        .map(|&i| {
            *remap.entry(i).or_insert_with(|| {
                out.push(verts[i as usize]);
                (out.len() - 1) as u32
            })
        })
        .collect();
    (Arc::new(out), new_inds)
}

struct MeshCpu {
    name: String,
    verts: Arc<Vec<Vertex>>,
    inds: Vec<u32>,
    submeshes: Vec<unity_formats::mesh::SubMesh>,
    index_size: u32,
}

/// The index run for one sub-mesh as absolute vertex indices; `None` if it is not a triangle list
/// or does not fit the mesh.
fn submesh_indices(m: &MeshCpu, sub: usize) -> Option<Vec<u32>> {
    if m.submeshes.is_empty() {
        return (sub == 0).then(|| m.inds.clone());
    }
    let sm = m.submeshes.get(sub)?;
    if sm.topology != 0 || m.index_size == 0 {
        return None;
    }
    let first = (sm.first_byte / m.index_size) as usize;
    let end = first.checked_add(sm.index_count as usize)?;
    let run = m.inds.get(first..end)?;
    let out: Vec<u32> = run.iter().map(|&i| i + sm.base_vertex).collect();
    if out.is_empty() || out.iter().any(|&i| i as usize >= m.verts.len()) {
        return None;
    }
    Some(out)
}

/// Read a material's base map as a plain 2D texture. A `Texture2DArray` (HDRP terrain-blend
/// materials keep their `_BaseMaps` in one) has no per-instance layer selector in the data the port
/// decodes, so its layer 0 is returned and the second value is `true`: callers count those
/// instances as an approximation rather than a resolved base map.
pub fn read_base_map(
    content: &GameContent,
    r: &ObjRef,
) -> Result<(unity_formats::texture::UnityTexture, bool)> {
    if content.player().class_name(r) != Some("Texture2DArray") {
        return Ok((content.read_texture(r)?, false));
    }
    use unity_formats::texture::{TextureFormat, UnityTexture};
    let t = content
        .player()
        .decode(r)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let int = |k: &str| {
        t.get(k)
            .and_then(|v| v.as_i64())
            .with_context(|| format!("Texture2DArray: no integer {k}"))
    };
    let (w, h) = (int("m_Width")? as u32, int("m_Height")? as u32);
    let (depth, mips) = (int("m_Depth")? as usize, int("m_MipCount")? as u32);
    // `m_Format` of a Texture2DArray is a `GraphicsFormat`, not a `TextureFormat`. Only the BC
    // block formats (96..=109 in that enum) are mapped; the byte-count check below is the
    // validation that the mapping is right for the file at hand.
    let graphics = int("m_Format")?;
    let (format, srgb_from_format) = match graphics {
        96 | 97 => (10, graphics == 96),
        100 | 101 => (12, graphics == 100),
        102 => (26, false),
        104 => (27, false),
        108 | 109 => (25, graphics == 108),
        other => bail!("Texture2DArray GraphicsFormat {other} is not mapped"),
    };
    let stream = t
        .get("m_StreamData")
        .map(unity_formats::resource::StreamingInfo::of)
        .transpose()
        .map_err(|e| anyhow::anyhow!("{e}"))?
        .flatten();
    let data = match stream {
        Some(s) => content
            .player()
            .read_stream(&r.file, &s)
            .map_err(|e| anyhow::anyhow!("{e}"))?,
        None => t
            .get("image data")
            .and_then(|v| v.as_bytes())
            .context("Texture2DArray: no image data")?
            .to_vec(),
    };
    let tf = TextureFormat::from_id(format).map_err(|e| anyhow::anyhow!("{e}"))?;
    ensure!(
        depth > 0 && mips > 0,
        "Texture2DArray without layers or mips"
    );
    ensure!(!tf.is_crunched(), "crunched Texture2DArray");
    let slice: usize = (0..mips)
        .map(|i| {
            tf.level_size((w >> i).max(1), (h >> i).max(1))
                .context("level size")
        })
        .sum::<Result<usize>>()?;
    ensure!(
        slice.checked_mul(depth) == Some(data.len()),
        "Texture2DArray data is {} bytes, {depth} layers of {slice} expected",
        data.len()
    );
    let base = UnityTexture {
        name: t
            .get("m_Name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned(),
        width: w,
        height: h,
        format,
        mips,
        srgb: srgb_from_format,
        usage: 0,
        wrap: [0, 0],
        filter: 1,
        mip_bias: 0.0,
        aniso: 1,
        data: data[..slice].to_vec(),
        unity: content
            .player()
            .file(&r.file)
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .unity,
    };
    Ok((base, true))
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
    let debug = std::env::var_os("RRISING_SCENE_DEBUG").is_some();

    // Reference tables. Failure here leaves every renderer as a marker, with the reason recorded.
    // Each ManagedRenderMeshArray is located in the managed blob by its observed layout (other
    // managed shared records sit before and after it and their length is not decoded), and keyed by
    // its 1-based serialized shared-component index.
    let sc = scene.shared_and_managed_components()?;
    let table = (|| -> Result<(Vec<(i32, ArrayTable)>, usize)> {
        let catalog = RuntimeContentCatalog::open(
            &data_dir.join("StreamingAssets/ContentArchives/archive_dependencies.bin"),
        )?;
        let ruo = find_referenced_objects(content, &catalog, scene_path)?;
        let (_, bytes) = content
            .player()
            .behaviour(&ruo)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let pptrs = parse_pptr_table(&bytes)?;
        let resolve = |i: i32| -> Option<ObjRef> {
            content
                .player()
                .resolve_pptr(&ruo.file, *pptrs.get(usize::try_from(i).ok()?)?)
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
        let mut out = Vec::new();
        let mut cursor = 0usize;
        for (j, rec) in sc.managed_shared_records.iter().enumerate() {
            if rec.stable_type_hash != MANAGED_RENDER_MESH_ARRAY_HASH {
                continue;
            }
            let mut found = None;
            for off in cursor..sc.managed_blob.len() {
                let Ok((a, b, end)) = parse_index_arrays(&sc.managed_blob[off..], pptrs.len())
                else {
                    continue;
                };
                if a.is_empty() || b.is_empty() {
                    continue;
                }
                let pair = match (class_of(&a).as_deref(), class_of(&b).as_deref()) {
                    (Some("Mesh"), Some("Material")) => (a, b),
                    (Some("Material"), Some("Mesh")) => (b, a),
                    _ => continue,
                };
                found = Some((pair, off + end));
                break;
            }
            let Some(((mesh_idx, mat_idx), end)) = found else {
                continue;
            };
            cursor = end;
            let meshes = mesh_idx
                .iter()
                .map(|&i| {
                    resolve(i).unwrap_or(ObjRef {
                        file: String::new(),
                        path_id: 0,
                    })
                })
                .collect();
            let materials = mat_idx.iter().map(|&i| resolve(i)).collect();
            let serialized = i32::try_from(sc.unmanaged.len() + j + 1)?;
            out.push((serialized, ArrayTable { meshes, materials }));
        }
        ensure!(
            !out.is_empty(),
            "no ManagedRenderMeshArray could be located in the managed blob"
        );
        Ok((out, pptrs.len()))
    })();
    let arrays: Vec<(i32, ArrayTable)> = match table {
        Ok((arrays, n)) => {
            stats.referenced_objects = n;
            arrays
        }
        Err(e) => {
            stats.note = format!("{e:#}");
            Vec::new()
        }
    };
    stats.render_mesh_arrays = arrays.len();

    // Chunk -> shared-component values. `shared_component_remap` holds, in chunk stream order, one
    // entry per shared (non-chunk-component) type of the chunk's archetype (the entry count equals
    // that sum in all 1,183 scenes). A chunk's ManagedRenderMeshArray is the table whose serialized
    // index appears among its values.
    let mut chunk_values: Vec<&[i32]> = Vec::with_capacity(resolved.chunks.len());
    let mut mapping_ok = true;
    let mut at = 0usize;
    for chunk in &resolved.chunks {
        let k = match resolved
            .archetypes
            .get(chunk.header.archetype_index as usize)
        {
            Some(Some(a)) => a
                .types
                .iter()
                .filter(|t| t.is_shared_component_type() && !t.is_chunk_component())
                .count(),
            _ => {
                mapping_ok = false;
                0
            }
        };
        let slice = sc
            .shared_component_remap
            .get(at..at + k)
            .unwrap_or_else(|| {
                mapping_ok = false;
                &[]
            });
        chunk_values.push(slice);
        at += k;
    }
    if mapping_ok {
        mapping_ok = at == sc.shared_component_remap.len();
    }
    let array_for_chunk = |ci: usize| -> Option<&ArrayTable> {
        if mapping_ok {
            let vals = chunk_values.get(ci)?;
            arrays
                .iter()
                .find(|(id, _)| vals.contains(id))
                .map(|(_, t)| t)
        } else if arrays.len() == 1 {
            // Mapping not decodable: only a lone array is unambiguous.
            Some(&arrays[0].1)
        } else {
            None
        }
    };

    // Per material, resolved once.
    let mut mat_cache: HashMap<ObjRef, Option<MatInfo>> = HashMap::new();
    let mut raw: HashMap<(ObjRef, u16, Option<ObjRef>, bool), RawGroup> = HashMap::new();
    let mut markers: Vec<Instance> = Vec::new();
    let mut marker_entities: Vec<Entity> = Vec::new();

    for (ci, chunk) in resolved.chunks.iter().enumerate() {
        let id_base = resolved
            .archetypes
            .get(chunk.header.archetype_index as usize)
            .and_then(|a| a.as_ref())
            .map(|a| a.data.layout.offsets[0] as usize);
        for e in 0..chunk.header.count as usize {
            stats.entities += 1;
            let id = id_base
                .and_then(|b| chunk.buffer.get(b + 8 * e..b + 8 * e + 8))
                .map(|b| {
                    Entity::new(
                        i32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                        i32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                    )
                })
                .unwrap_or(Entity::NULL);
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
            if needs_runtime_or_range_resolution(info) {
                stats.unresolved_runtime_or_range += 1;
                {
                    markers.push(inst);
                    marker_entities.push(id);
                }
                continue;
            }
            let Some(table) = array_for_chunk(ci) else {
                stats.unresolved_no_array += 1;
                {
                    markers.push(inst);
                    marker_entities.push(id);
                }
                continue;
            };
            let mesh_ref = usize::try_from(info.mesh_array_index())
                .ok()
                .and_then(|i| table.meshes.get(i))
                .filter(|r| r.path_id != 0);
            let Some(mesh_ref) = mesh_ref else {
                stats.unresolved_index += 1;
                {
                    markers.push(inst);
                    marker_entities.push(id);
                }
                continue;
            };
            let material = usize::try_from(info.material_array_index())
                .ok()
                .and_then(|i| table.materials.get(i))
                .cloned()
                .flatten();
            let mat = material.and_then(|m| {
                mat_cache
                    .entry(m.clone())
                    .or_insert_with(|| {
                        let data = content.read_material(&m).ok()?;
                        let (surface, summary) = classify_material(&data);
                        let texture = rrising_core::base_map_name(&data).and_then(|slot| {
                            data.textures
                                .iter()
                                .find(|(n, _)| n == slot)
                                .and_then(|(_, env)| env.texture.clone())
                        });
                        if debug {
                            eprintln!("material {summary} -> {surface:?}");
                        }
                        Some(MatInfo { texture, surface })
                    })
                    .clone()
            });
            let (texture, surface) = match &mat {
                Some(m) => (m.texture.clone(), m.surface),
                None => (None, Surface::Opaque),
            };
            match surface {
                Surface::Opaque | Surface::Cutout => {}
                Surface::Transparent => {
                    stats.excluded_transparent += 1;
                    continue;
                }
            }
            let g = raw
                .entry((
                    mesh_ref.clone(),
                    info.sub_mesh(),
                    texture.clone(),
                    surface == Surface::Cutout,
                ))
                .or_insert_with(|| RawGroup {
                    cutout: surface == Surface::Cutout,
                    texture,
                    instances: vec![],
                    entities: vec![],
                });
            g.instances.push(inst);
            g.entities.push(id);
        }
    }

    // Read each unique mesh once; a group whose mesh or sub-mesh cannot be used becomes markers
    // (unreadable/skinned) or an excluded, counted instance (not a triangle list).
    let mut cpu: HashMap<ObjRef, Option<MeshCpu>> = HashMap::new();
    let mut sub_ids: HashMap<(ObjRef, u16), Option<SubMeshGpu>> = HashMap::new();
    let mut groups = Vec::new();
    let mut keys: Vec<_> = raw.into_iter().collect();
    keys.sort_by(|a, b| a.0.cmp(&b.0));
    for ((mesh_ref, sub, _, _), g) in keys {
        let mesh = cpu.entry(mesh_ref.clone()).or_insert_with(|| {
            let m = content.read_mesh(&mesh_ref).ok()?;
            if m.skin.is_some() {
                return None;
            }
            let (v, i) = mesh::build_vertices(&m).ok()?;
            if v.is_empty() || i.is_empty() {
                return None;
            }
            Some(MeshCpu {
                name: m.name.clone(),
                verts: Arc::new(v),
                inds: i,
                submeshes: m.submeshes.clone(),
                index_size: m.index_size,
            })
        });
        let Some(mesh) = mesh.as_ref() else {
            stats.unresolved_mesh_read += g.instances.len();
            markers.extend(g.instances);
            marker_entities.extend(g.entities);
            continue;
        };
        let next_id = sub_ids.len();
        let entry = sub_ids.entry((mesh_ref.clone(), sub)).or_insert_with(|| {
            let inds = submesh_indices(mesh, sub as usize)?;
            let (verts, inds) = if mesh.submeshes.len() > 1 {
                compact_vertices(&mesh.verts, &inds)
            } else {
                (mesh.verts.clone(), inds)
            };
            Some((next_id, verts, Arc::new(inds)))
        });
        match entry {
            Some((id, verts, inds)) => {
                if mesh.submeshes.len() > 1 {
                    stats.submesh_instances += g.instances.len();
                }
                groups.push(ReadyGroup {
                    name: if mesh.submeshes.len() > 1 {
                        format!("{}#{sub}", mesh.name)
                    } else {
                        mesh.name.clone()
                    },
                    mesh_id: *id,
                    verts: verts.clone(),
                    inds: inds.clone(),
                    texture: g.texture,
                    cutout: g.cutout,
                    instances: g.instances,
                    entities: g.entities,
                });
            }
            None => stats.excluded_submesh += g.instances.len(),
        }
    }
    stats.unique_meshes_resolved = cpu.values().filter(|v| v.is_some()).count();
    stats.unique_textures = groups
        .iter()
        .filter_map(|g| g.texture.as_ref())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    stats.drawn_mesh_instances = groups.iter().map(|g| g.instances.len()).sum();
    stats.cutout_instances = groups
        .iter()
        .filter(|g| g.cutout)
        .map(|g| g.instances.len())
        .sum();
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
        marker_entities,
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
             (runtime/range {}, bad index {}, no array for chunk {}, mesh unreadable/skinned {}) | {} render-mesh array(s), {} instances on a sub-mesh of a multi-sub-mesh mesh | \
             excluded: transparent {}, unusable sub-mesh {} | alpha-tested instances drawn {} | base map upload failed -> grey fallback: {} instances ({} textures) | Texture2DArray base map shown as layer 0: {} instances{}",
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
            self.unresolved_no_array,
            self.unresolved_mesh_read,
            self.render_mesh_arrays,
            self.submesh_instances,
            self.excluded_transparent,
            self.excluded_submesh,
            self.cutout_instances,
            self.texture_fallback_instances,
            self.texture_fallback_textures,
            self.array_layer0_instances,
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

/// A scene built as a `World` and stepped by the `Schedule` (`TransformSystem`).
pub struct SimWorld {
    pub world: rrising_ecs::World,
    schedule: rrising_ecs::Schedule,
    pub ltw: rrising_ecs::TypeIndex,
    /// Wall time of building the world from the decoded scene.
    pub build_ms: f64,
    /// Wall time of each fixed frame, in order.
    pub frame_ms: Vec<f64>,
    /// Entities the last frame wrote / the first frame wrote.
    pub written_first: usize,
    pub written_last: usize,
    pub steering_types_present: bool,
    pub steering_live_candidates: usize,
    pub steering_input_updates: usize,
}

const SCALE_HASH: u64 = 0xadc0_56c7_729e_d01e;
const POST_HASH: u64 = 0x0233_3773_b807_e90d;
pub const STEP_NS: u64 = 16_666_667;

/// Instantiate a partial clone of the pinned client player prefab in a scene world.
/// Rendering, animation, locomotion and external prefab references are not supplied.
pub fn spawn_player(
    world: &mut rrising_ecs::World,
    data: &Path,
    layouts_path: &Path,
) -> Result<Entity> {
    use rrising_content::prefab_catalog::ScenePrefabCatalog;
    use std::collections::BTreeSet;

    let layouts = TypeLayouts::from_json(&std::fs::read(layouts_path)?)?;
    let scene = SceneFile::open(
        &data.join("StreamingAssets/EntityScenes/295c92b7a799a3443b40e9ef192b5727.0.entities"),
    )?;
    let mut catalog =
        ScenePrefabCatalog::read_linked_templates(&scene, &layouts, &BTreeSet::from([38526109]))?;
    let template = catalog
        .linked_templates
        .remove(&38526109)
        .context("client player prefab not found")?;
    let entities = template.instantiate(&mut world.store, &mut world.types, &layouts)?;
    let player = world
        .types
        .type_index_of("ProjectM.PlayerCharacter")
        .context("player type missing")?;
    let input = world
        .types
        .type_index_of("ProjectM.EntityInput")
        .context("player input type missing")?;
    let candidates: Vec<_> = world
        .store
        .query_entities(&rrising_ecs::EntityQuery {
            all: vec![player, input],
            ..Default::default()
        })?
        .into_iter()
        .filter(|entity| entities.contains(entity))
        .collect();
    anyhow::ensure!(
        candidates.len() == 1,
        "expected one live player input in cloned prefab, found {}",
        candidates.len()
    );
    println!("Spawned partial client player prefab: {} linked entities, input target {:?}; original visual binding and locomotion are not implemented.", entities.len(), candidates[0]);
    Ok(candidates[0])
}

/// Place a cloned player at caller-supplied world coordinates for diagnostics.
/// This does not select an original spawn point or perform collision checks.
/// Run the transform schedule after calling it to update LocalToWorld.
pub fn set_diagnostic_player_position(
    world: &mut rrising_ecs::World,
    player: Entity,
    position: [f32; 3],
) -> Result<()> {
    ensure!(
        position.iter().all(|v| v.is_finite()),
        "player position must be finite"
    );
    let player_type = world
        .types
        .type_index_of("ProjectM.PlayerCharacter")
        .context("player type missing")?;
    ensure!(
        world
            .store
            .try_component_bytes(player, player_type)?
            .is_some(),
        "entity is not a player"
    );
    let translation = world
        .types
        .type_index_of("Unity.Transforms.Translation")
        .context("translation type missing")?;
    ensure!(
        world.store.component_bytes(player, translation)?.len() == 12,
        "invalid Translation layout"
    );
    let bytes: Vec<_> = position.into_iter().flat_map(f32::to_le_bytes).collect();
    world
        .store
        .set_component_bytes(player, translation, &bytes)?;
    Ok(())
}

/// Build a scene World and run fixed frames. Shared steering, if live candidates
/// exist, updates EntityInput before transforms update LocalToWorld. No locomotion,
/// navigation service or original game-loop ordering is supplied by this driver.
pub fn simulate(layouts_path: &Path, scene_path: &Path, frames: usize) -> Result<SimWorld> {
    use rrising_content::type_layouts::{LOCAL_TRANSFORM_HASH, ROTATION_HASH, TRANSLATION_HASH};
    use rrising_content::world_adapter::{build_scene_world, PARENT_HASH};
    use rrising_core::movement::{SharedSteeringSystem, SharedSteeringTypes};
    use rrising_ecs::transform::{TransformSystem, TransformTypes};
    use rrising_ecs::Schedule;
    let layouts = TypeLayouts::from_json(&std::fs::read(layouts_path)?)?;
    let scene = SceneFile::open(scene_path)?;
    let t = std::time::Instant::now();
    let mut sw = build_scene_world(&layouts, &scene)?;
    let build_ms = t.elapsed().as_secs_f64() * 1e3;
    let ty = |h: u64| sw.type_of_hash.get(&h).copied();
    let ltw = ty(LOCAL_TO_WORLD_HASH).context("scene has no LocalToWorld")?;
    let tt = TransformTypes {
        local_to_world: ltw,
        local_transform: ty(LOCAL_TRANSFORM_HASH),
        translation: ty(TRANSLATION_HASH),
        rotation: ty(ROTATION_HASH),
        scale: ty(SCALE_HASH),
        post_transform: ty(POST_HASH),
        parent: ty(PARENT_HASH),
    };
    let steering = SharedSteeringTypes::from_hashes(&sw.type_of_hash);
    let steering_live_candidates = steering
        .as_ref()
        .map_or(0, |t| sw.world.query_count(&t.query()));
    let mut sched = Schedule::new(STEP_NS)?;
    // The original has RequireMatchingQueriesForUpdate. This fixed-content driver
    // never creates entities, so register only if the initial live query is nonempty.
    if let Some(types) = steering.filter(|_| steering_live_candidates != 0) {
        sched.add_group("SetInput")?;
        sched.add_system("SetInput", Box::new(SharedSteeringSystem::new(types)))?;
    }
    sched.add_group("ExperimentalPlayer")?;
    sched.add_group("Simulation")?;
    sched.add_system("Simulation", Box::new(TransformSystem::new(tt)))?;
    let mut frame_ms = Vec::with_capacity(frames);
    let (mut written_first, mut written_last) = (0, 0);
    let mut steering_input_updates = 0;
    for i in 0..frames {
        let t = std::time::Instant::now();
        sched.run_frame(&mut sw.world)?;
        frame_ms.push(t.elapsed().as_secs_f64() * 1e3);
        steering_input_updates += sched
            .system::<SharedSteeringSystem>(SharedSteeringSystem::NAME)
            .map_or(0, |s| s.entities_updated);
        let w = sched
            .system::<TransformSystem>(TransformSystem::NAME)
            .context("system")?
            .stats()
            .entities_written();
        if i == 0 {
            written_first = w;
        }
        written_last = w;
    }
    Ok(SimWorld {
        world: sw.world,
        schedule: sched,
        ltw,
        build_ms,
        frame_ms,
        written_first,
        written_last,
        steering_types_present: steering.is_some(),
        steering_live_candidates,
        steering_input_updates,
    })
}

impl SimWorld {
    /// Install an explicitly approximate, single-player movement driver.
    pub fn enable_experimental_player_motion(&mut self, entity: Entity) -> Result<()> {
        let types =
            rrising_core::ordinary_movement_ecs::OrdinaryMovementTypes::from_world(&self.world)?;
        let input = self
            .world
            .types
            .type_index_of("ProjectM.EntityInput")
            .context("player input missing")?;
        // Validate all accessed fields before modifying the schedule.
        self.world.store.component_bytes(entity, input)?;
        self.world.store.component_bytes(entity, types.movement)?;
        self.world.store.component_bytes(entity, types.velocity)?;
        self.world
            .store
            .component_bytes(entity, types.move_velocity)?;
        self.world
            .store
            .component_bytes(entity, types.translation)?;
        self.schedule.add_system(
            "ExperimentalPlayer",
            Box::new(ExperimentalPlayerMotion {
                entity,
                input,
                types,
            }),
        )
    }
    /// Advance a retained world on fixed steps; the step cap limits catch-up after a pause.
    pub fn advance(&mut self, elapsed_ns: u64, max_steps: u32) -> Result<u32> {
        self.schedule
            .advance(&mut self.world, elapsed_ns, max_steps)
    }

    pub fn instance(&self, e: Entity) -> Option<Instance> {
        let b = self.world.store.try_component_bytes(e, self.ltw).ok()??;
        let mut m = [0f32; 16];
        for (v, c) in m.iter_mut().zip(b.as_chunks::<4>().0) {
            *v = f32::from_le_bytes(*c);
        }
        matrix_instance(m)
    }

    /// Replace every instance's matrix with the one now in the world. Returns
    /// (read back, missing); a missing instance keeps its stored matrix.
    pub fn apply(&self, data: &mut SceneData) -> (usize, usize) {
        let (mut ok, mut missing) = (0, 0);
        let mut fix = |inst: &mut Instance, e: Entity| match self.instance(e) {
            Some(i) => {
                *inst = i;
                ok += 1;
            }
            None => missing += 1,
        };
        for g in &mut data.groups {
            for (inst, &e) in g.instances.iter_mut().zip(&g.entities) {
                fix(inst, e);
            }
        }
        for (inst, &e) in data.markers.iter_mut().zip(&data.marker_entities) {
            fix(inst, e);
        }
        (ok, missing)
    }
}

/// Deliberate approximation: invokes recovered leaf bodies in a host-defined order.
/// It is not the original command scheduler, and excludes abilities and collisions.
struct ExperimentalPlayerMotion {
    entity: Entity,
    input: rrising_ecs::TypeIndex,
    types: rrising_core::ordinary_movement_ecs::OrdinaryMovementTypes,
}

impl rrising_ecs::System for ExperimentalPlayerMotion {
    fn name(&self) -> &str {
        "ExperimentalPlayerMotion"
    }
    fn update(
        &mut self,
        world: &mut rrising_ecs::World,
        ctx: &rrising_ecs::FrameContext,
    ) -> Result<()> {
        let input = world
            .store
            .component_bytes(self.entity, self.input)?
            .to_vec();
        let mut movement = world
            .store
            .component_bytes(self.entity, self.types.movement)?
            .to_vec();
        rrising_core::movement::setup_ability_movement(&mut movement, &input)?;
        world
            .store
            .set_component_bytes(self.entity, self.types.movement, &movement)?;
        let mut command = [0u8; rrising_core::command_movement::INPUT_COMMAND_SIZE];
        rrising_core::command_movement::write_command_input_from_world(
            world,
            self.entity,
            &mut command,
            self.input,
        )?;
        command[144..148].copy_from_slice(&(ctx.delta_ns as f32 * 1e-9).to_le_bytes());
        rrising_core::ordinary_movement_ecs::step_ordinary_command_bytes(
            world,
            self.entity,
            self.types,
            &command,
            true,
        )
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Attach a diagnostic, static-pose candidate model to the selected entity.
/// Its geometry is in prefab-root space and its group instances follow the
/// entity's LocalToWorld. Hybrid model selection and animation remain unknown.
pub fn attach_player_visual(
    content: &GameContent,
    data: &mut SceneData,
    entity: Entity,
    instance: Instance,
) -> Result<usize> {
    use rrising_render::skin::skin_static_pose;
    use unity_formats::scene::Scene;

    let scene = Scene::read(content.player(), "ae19a255904fd765632aa97a2d7c670c")?;
    let roots: Vec<_> = scene
        .game_objects
        .iter()
        .filter(|(_, object)| object.name == "HYB_VampireMale_Prefab")
        .map(|(id, _)| *id)
        .collect();
    ensure!(roots.len() == 1, "candidate model root is ambiguous");
    let root = scene
        .transform_of(roots[0])
        .context("candidate model root transform")?;
    let root_world = scene.world_of(root)?;
    for (row, values) in root_world.iter().enumerate() {
        for (col, value) in values.iter().enumerate() {
            ensure!(
                (*value - f64::from(u8::from(row == col))).abs() < 1e-6,
                "candidate model root is not identity; prefab-space conversion required"
            );
        }
    }
    let (renderers, _) = scene.renderers(content.player())?;
    let first_mesh = data
        .groups
        .iter()
        .map(|g| g.mesh_id)
        .max()
        .map_or(0, |id| id + 1);
    let mut count = 0;
    for source in renderers
        .into_iter()
        .filter(|r| r.path.starts_with("HYB_VampireMale_Prefab/") && r.skin.is_some())
    {
        let skin = source.skin.context("skin")?;
        let joints = skin
            .bones
            .iter()
            .map(|id| Ok(scene.world_of(id.context("missing bone")?)?))
            .collect::<Result<Vec<_>>>()?;
        let mesh = content.read_mesh(&source.mesh)?;
        let (vertices, indices) = skin_static_pose(&mesh, &joints)?;
        data.groups.push(ReadyGroup {
            name: format!("experimental player {}", mesh.name),
            mesh_id: first_mesh + count,
            verts: Arc::new(vertices),
            inds: Arc::new(indices),
            texture: None,
            cutout: false,
            instances: vec![instance],
            entities: vec![entity],
        });
        count += 1;
    }
    ensure!(count > 0, "candidate player model has no skinned renderers");
    Ok(count)
}

/// Precompute samples of the first base-controller clip for the diagnostic player.
/// This is not the game's animation state selection, blending, or GPU skinning.
pub fn player_walk_samples(
    content: &GameContent,
    sample_count: usize,
) -> Result<(f32, Vec<Vec<Vec<Vertex>>>)> {
    use rrising_render::{clip_pose::sample_bones, skin::skin_static_pose};
    use unity_formats::{
        clip::{controller_clips, ClipData},
        scene::Scene,
    };
    ensure!(sample_count > 0, "animation needs samples");
    let scene = Scene::read(content.player(), "ae19a255904fd765632aa97a2d7c670c")?;
    let animator = scene
        .animators(content.player())?
        .into_iter()
        .find(|a| a.path.starts_with("HYB_VampireMale_Prefab/"))
        .context("player animator")?;
    let root = scene
        .transform_of(animator.game_object)
        .context("animator root")?;
    let controller = animator.controller.context("controller")?;
    let data = content.player().decode(&controller)?;
    let base = match data.get("m_Controller") {
        Some(ptr) => content
            .player()
            .resolve(&controller.file, ptr)?
            .context("base controller")?,
        None => controller,
    };
    let clips = controller_clips(content.player(), &base)?;
    let clip = ClipData::read(content.player(), clips.first().context("no clip")?)?;
    ensure!(
        clip.name == "VampireMale_Unarmed_Walk_Forward" && clip.duration > 0.0,
        "unexpected walk clip"
    );
    let (renderers, _) = scene.renderers(content.player())?;
    let mut result = Vec::new();
    for renderer in renderers
        .into_iter()
        .filter(|r| r.path.starts_with("HYB_VampireMale_Prefab/") && r.skin.is_some())
    {
        let skin = renderer.skin.context("skin")?;
        let bones = skin
            .bones
            .iter()
            .map(|id| id.context("missing bone"))
            .collect::<Result<Vec<_>>>()?;
        let mesh = content.read_mesh(&renderer.mesh)?;
        let mut frames = Vec::with_capacity(sample_count);
        for index in 0..sample_count {
            let joints = sample_bones(
                &scene,
                root,
                &bones,
                &clip,
                clip.duration * index as f32 / sample_count as f32,
            )?;
            frames.push(skin_static_pose(&mesh, &joints)?.0);
        }
        result.push(frames);
    }
    ensure!(!result.is_empty(), "player has no skinned parts");
    Ok((clip.duration, result))
}

#[cfg(test)]
mod managed_array_tests {
    use super::{needs_runtime_or_range_resolution, parse_index_arrays};
    use rrising_ecs::MaterialMeshInfo;

    #[test]
    fn merged_static_sections_preserve_distinct_meshes_and_bounds() {
        use super::{append_static_section, ReadyGroup, SceneData, SceneStats};
        use std::sync::Arc;
        let section = |x: f32| SceneData {
            groups: vec![ReadyGroup {
                name: "mesh".into(),
                mesh_id: 0,
                verts: Arc::new(vec![]),
                inds: Arc::new(vec![]),
                texture: None,
                cutout: false,
                instances: vec![],
                entities: vec![],
            }],
            markers: vec![],
            marker_entities: vec![],
            stats: SceneStats::default(),
            bounds: ([x, 0., 0.], [x + 1., 1., 1.]),
        };
        let mut first = section(0.);
        append_static_section(&mut first, section(10.));
        assert_eq!(
            first.groups.iter().map(|g| g.mesh_id).collect::<Vec<_>>(),
            [0, 1]
        );
        assert_eq!(first.bounds, ([0., 0., 0.], [11., 1., 1.]));
    }

    #[test]
    fn terrain_placement_transforms_instances_and_camera_bounds() {
        use super::{place_terrain_section, SceneData, SceneStats};
        use rrising_math::{float3, float4x4};
        use rrising_render::Instance;
        let mut data = SceneData {
            groups: vec![],
            markers: vec![Instance::from_matrix(&float4x4::translate(float3::new(
                1., 2., 3.,
            )))],
            marker_entities: vec![],
            stats: SceneStats::default(),
            bounds: ([0.; 3], [0.; 3]),
        };
        place_terrain_section(&mut data, float4x4::translate(float3::new(-20., 0., 40.)));
        assert_eq!(&data.markers[0].model[3][..3], &[-19., 2., 43.]);
        assert_eq!(data.bounds, ([-19., 2., 43.], [-19., 2., 43.]));
    }

    #[test]
    fn live_scene_retains_schedule_and_applies_updated_transforms() {
        use super::{SceneData, SceneStats, SimWorld};
        use rrising_ecs::transform::{TransformSystem, TransformTypes};
        use rrising_ecs::{
            ArchetypeInput, ChunkInput, EntityIds, Schedule, SpecialTypes, TypeCategory,
            TypeManager, TypeSpec, World, WorldInput,
        };
        let mut types = TypeManager::default();
        types.register(TypeSpec::tag("<reserved>")).unwrap();
        let mut entity_spec = TypeSpec::component_data("Entity", 8, 8);
        entity_spec.category = TypeCategory::EntityData;
        let entity_type = types.register(entity_spec).unwrap();
        let position = types
            .register(TypeSpec::component_data("Translation", 12, 4))
            .unwrap();
        let ltw = types
            .register(TypeSpec::component_data("LocalToWorld", 64, 4))
            .unwrap();
        let floats = |v: [f32; 3]| v.into_iter().flat_map(f32::to_le_bytes).collect::<Vec<_>>();
        let input = WorldInput {
            archetypes: vec![ArchetypeInput {
                types: vec![position, ltw],
            }],
            chunks: vec![ChunkInput {
                archetype: 0,
                entities: EntityIds::Fresh(1),
                components: vec![(position, floats([1., 2., 3.]))],
                shared_components: vec![],
            }],
        };
        let world = World::build(types, SpecialTypes::new(entity_type), 4, &input).unwrap();
        let entity = world.entities[0];
        let mut schedule = Schedule::new(10).unwrap();
        schedule.add_group("simulation").unwrap();
        schedule
            .add_system(
                "simulation",
                Box::new(TransformSystem::new(TransformTypes {
                    local_to_world: ltw,
                    local_transform: None,
                    translation: Some(position),
                    rotation: None,
                    scale: None,
                    post_transform: None,
                    parent: None,
                })),
            )
            .unwrap();
        let mut sim = SimWorld {
            world,
            schedule,
            ltw,
            build_ms: 0.,
            frame_ms: vec![],
            written_first: 0,
            written_last: 0,
            steering_types_present: false,
            steering_live_candidates: 0,
            steering_input_updates: 0,
        };
        let marker = super::matrix_instance([
            1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.,
        ])
        .unwrap();
        let mut scene = SceneData {
            groups: vec![],
            markers: vec![marker],
            marker_entities: vec![entity],
            stats: SceneStats::default(),
            bounds: ([0.; 3], [1.; 3]),
        };
        assert_eq!(sim.advance(9, 4).unwrap(), 0);
        assert_eq!(sim.advance(1, 4).unwrap(), 1);
        assert_eq!(sim.apply(&mut scene), (1, 0));
        assert_eq!(scene.markers[0].model[3][..3], [1., 2., 3.]);
        sim.world
            .store
            .set_component_bytes(entity, position, &floats([4., 5., 6.]))
            .unwrap();
        assert_eq!(sim.advance(10, 4).unwrap(), 1);
        sim.apply(&mut scene);
        assert_eq!(scene.markers[0].model[3][..3], [4., 5., 6.]);
        assert_eq!(sim.advance(1000, 4).unwrap(), 4);
        assert_eq!(sim.advance(0, 4).unwrap(), 0);
    }

    #[test]
    fn requires_both_mesh_and_material_to_be_static() {
        assert!(!needs_runtime_or_range_resolution(
            MaterialMeshInfo::from_render_mesh_array_indices(0, 0, 0)
        ));
        assert!(needs_runtime_or_range_resolution(MaterialMeshInfo::new(
            3, -1, 0
        )));
        assert!(needs_runtime_or_range_resolution(MaterialMeshInfo::new(
            -1, 3, 0
        )));
        assert!(needs_runtime_or_range_resolution(
            MaterialMeshInfo::from_material_mesh_index_range(0, 1).unwrap()
        ));
    }

    #[test]
    fn object_indices_accept_null_and_reject_invalid_references() {
        let mut blob = vec![1, 1];
        blob.extend_from_slice(&2i32.to_le_bytes());
        blob.extend_from_slice(&(-1i32).to_le_bytes());
        blob.extend_from_slice(&1i32.to_le_bytes());
        blob.push(1);
        blob.extend_from_slice(&1i32.to_le_bytes());
        blob.extend_from_slice(&0i32.to_le_bytes());
        assert_eq!(
            parse_index_arrays(&blob, 2).unwrap(),
            (vec![-1, 1], vec![0], blob.len())
        );
        blob[6..10].copy_from_slice(&(-2i32).to_le_bytes());
        assert!(parse_index_arrays(&blob, 2).is_err());
        blob[6..10].copy_from_slice(&2i32.to_le_bytes());
        assert!(parse_index_arrays(&blob, 2).is_err());
    }

    #[test]
    #[ignore = "requires pinned client data and RRISING_LAYOUT_REPORT"]
    fn spawn_player_into_live_scene() {
        use super::{set_diagnostic_player_position, simulate, spawn_player, STEP_NS};
        use std::path::PathBuf;
        let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("VRISING_DATA"));
        let layouts = PathBuf::from(
            std::env::var_os("RRISING_LAYOUT_REPORT").expect("RRISING_LAYOUT_REPORT"),
        );
        let scene =
            data.join("StreamingAssets/EntityScenes/c6d34432693f24046ae5916650cc70ef.0.entities");
        let mut sim = simulate(&layouts, &scene, 0).unwrap();
        let entity = spawn_player(&mut sim.world, &data, &layouts).unwrap();
        let input = sim
            .world
            .types
            .type_index_of("ProjectM.EntityInput")
            .unwrap();
        rrising_core::player_input::write_player_movement(
            &mut sim.world,
            entity,
            input,
            [0.25, -0.5],
        )
        .unwrap();
        let position: Vec<u8> = [12.0f32, 3.0, -7.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        assert!(
            set_diagnostic_player_position(&mut sim.world, entity, [f32::NAN, 3., -7.]).is_err()
        );
        set_diagnostic_player_position(&mut sim.world, entity, [12., 3., -7.]).unwrap();
        sim.advance(STEP_NS, 1).unwrap();
        let bytes = sim.world.store.component_bytes(entity, input).unwrap();
        assert_eq!(&bytes[..4], &0.25f32.to_le_bytes());
        assert_eq!(&bytes[4..8], &(-0.5f32).to_le_bytes());
        let matrix = sim.world.store.component_bytes(entity, sim.ltw).unwrap();
        assert_eq!(&matrix[48..60], &position);
    }

    #[test]
    #[ignore = "requires pinned client data and RRISING_LAYOUT_REPORT"]
    fn experimental_motion_advances_player_on_fixed_steps() {
        use super::{simulate, spawn_player, STEP_NS};
        use std::path::PathBuf;
        let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("VRISING_DATA"));
        let layouts = PathBuf::from(
            std::env::var_os("RRISING_LAYOUT_REPORT").expect("RRISING_LAYOUT_REPORT"),
        );
        let scene =
            data.join("StreamingAssets/EntityScenes/c6d34432693f24046ae5916650cc70ef.0.entities");
        let mut sim = simulate(&layouts, &scene, 0).unwrap();
        let player = spawn_player(&mut sim.world, &data, &layouts).unwrap();
        sim.enable_experimental_player_motion(player).unwrap();
        let input = sim
            .world
            .types
            .type_index_of("ProjectM.EntityInput")
            .unwrap();
        rrising_core::player_input::write_player_movement(&mut sim.world, player, input, [1., 0.])
            .unwrap();
        let translation = sim
            .world
            .types
            .type_index_of("Unity.Transforms.Translation")
            .unwrap();
        let read_x = |world: &rrising_ecs::World| {
            f32::from_le_bytes(
                world.store.component_bytes(player, translation).unwrap()[..4]
                    .try_into()
                    .unwrap(),
            )
        };
        let start = read_x(&sim.world);
        assert_eq!(sim.advance(STEP_NS * 3, 4).unwrap(), 3);
        let moved = read_x(&sim.world);
        assert!((moved - start - 4.0 * 3.0 * STEP_NS as f32 * 1e-9).abs() < 0.001);
        rrising_core::player_input::write_player_movement(&mut sim.world, player, input, [0., 0.])
            .unwrap();
        sim.advance(STEP_NS * 2, 4).unwrap();
        assert_eq!(read_x(&sim.world), moved);
        assert!((sim.instance(player).unwrap().model[3][0] - moved).abs() < 0.0001);
    }

    #[test]
    #[ignore = "requires pinned client data and RRISING_LAYOUT_REPORT"]
    fn diagnostic_key_press_and_release_drive_player() {
        use super::{simulate, spawn_player, STEP_NS};
        use crate::input_control::{record_key, record_player_key, step_keys, write_input};
        use std::{collections::HashSet, path::PathBuf};
        use winit::{event::ElementState, keyboard::KeyCode};
        let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("VRISING_DATA"));
        let layouts = PathBuf::from(
            std::env::var_os("RRISING_LAYOUT_REPORT").expect("RRISING_LAYOUT_REPORT"),
        );
        let scene =
            data.join("StreamingAssets/EntityScenes/c6d34432693f24046ae5916650cc70ef.0.entities");
        let mut sim = simulate(&layouts, &scene, 0).unwrap();
        let player = spawn_player(&mut sim.world, &data, &layouts).unwrap();
        sim.enable_experimental_player_motion(player).unwrap();
        let input = sim
            .world
            .types
            .type_index_of("ProjectM.EntityInput")
            .unwrap();
        let translation = sim
            .world
            .types
            .type_index_of("Unity.Transforms.Translation")
            .unwrap();
        let read_z = |world: &rrising_ecs::World| {
            f32::from_le_bytes(
                world.store.component_bytes(player, translation).unwrap()[8..12]
                    .try_into()
                    .unwrap(),
            )
        };
        let mut keys = HashSet::new();
        let mut last = [0.; 3];
        let before = read_z(&sim.world);
        record_key(&mut keys, KeyCode::KeyW, ElementState::Pressed);
        write_input(&mut sim.world, player, input, &keys, 0., &mut last).unwrap();
        sim.advance(STEP_NS * 3, 4).unwrap();
        let moved = read_z(&sim.world);
        assert!(moved > before);
        record_key(&mut keys, KeyCode::KeyW, ElementState::Released);
        write_input(&mut sim.world, player, input, &keys, 0., &mut last).unwrap();
        sim.advance(STEP_NS * 2, 4).unwrap();
        assert_eq!(read_z(&sim.world), moved);

        // Both OS events can arrive before a redraw: retain one simulation step.
        let mut pending = HashSet::new();
        record_player_key(
            &mut keys,
            &mut pending,
            KeyCode::KeyW,
            ElementState::Pressed,
        );
        record_player_key(
            &mut keys,
            &mut pending,
            KeyCode::KeyW,
            ElementState::Released,
        );
        write_input(
            &mut sim.world,
            player,
            input,
            &step_keys(&keys, &pending),
            0.,
            &mut last,
        )
        .unwrap();
        sim.advance(STEP_NS, 1).unwrap();
        let tapped = read_z(&sim.world);
        assert!(tapped > moved);
        pending.clear();
        write_input(
            &mut sim.world,
            player,
            input,
            &step_keys(&keys, &pending),
            0.,
            &mut last,
        )
        .unwrap();
        sim.advance(STEP_NS, 1).unwrap();
        assert_eq!(read_z(&sim.world), tapped);
    }

    #[test]
    #[ignore = "requires pinned client data and RRISING_LAYOUT_REPORT"]
    fn experimental_player_visual_follows_entity_transform() {
        use super::{
            attach_player_visual, player_walk_samples, simulate, spawn_player, SceneData,
            SceneStats, STEP_NS,
        };
        use rrising_content::{list_archives, GameContent};
        use std::path::{Path, PathBuf};
        let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("VRISING_DATA"));
        let layouts = PathBuf::from(
            std::env::var_os("RRISING_LAYOUT_REPORT").expect("RRISING_LAYOUT_REPORT"),
        );
        let scene =
            data.join("StreamingAssets/EntityScenes/c6d34432693f24046ae5916650cc70ef.0.entities");
        let archives = list_archives(&data.join("StreamingAssets/ContentArchives")).unwrap();
        let content =
            GameContent::open(&data, &archives, Path::new("../../assets/typetrees.tpk")).unwrap();
        let mut sim = simulate(&layouts, &scene, 0).unwrap();
        let entity = spawn_player(&mut sim.world, &data, &layouts).unwrap();
        sim.advance(STEP_NS, 1).unwrap();
        let mut visual = SceneData {
            groups: vec![],
            markers: vec![],
            marker_entities: vec![],
            stats: SceneStats::default(),
            bounds: ([0.; 3], [0.; 3]),
        };
        let count =
            attach_player_visual(&content, &mut visual, entity, sim.instance(entity).unwrap())
                .unwrap();
        assert_eq!(count, 2);
        let (duration, samples) = player_walk_samples(&content, 4).unwrap();
        assert!(duration > 0.0);
        assert_eq!(samples.len(), count);
        for (part, frames) in samples.iter().enumerate() {
            assert_eq!(frames.len(), 4);
            assert!(frames
                .iter()
                .all(|frame| frame.len() == visual.groups[part].verts.len()));
            assert!(frames[0]
                .iter()
                .zip(&frames[1])
                .any(|(a, b)| a.position != b.position));
        }
        let translation = sim
            .world
            .types
            .type_index_of("Unity.Transforms.Translation")
            .unwrap();
        let position: Vec<u8> = [12.0f32, 3.0, -7.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        sim.world
            .store
            .set_component_bytes(entity, translation, &position)
            .unwrap();
        sim.advance(STEP_NS, 1).unwrap();
        assert_eq!(sim.apply(&mut visual), (2, 0));
        for group in &visual.groups {
            assert_eq!(group.entities, vec![entity]);
            assert_eq!(&group.instances[0].model[3][..3], &[12., 3., -7.]);
        }
    }

    #[test]
    #[ignore = "requires pinned client data and a GPU"]
    fn original_walk_samples_update_the_gpu_render() {
        use super::{attach_player_visual, player_walk_samples, SceneData, SceneStats};
        use rrising_content::{list_archives, GameContent};
        use rrising_ecs::Entity;
        use rrising_math::{float3, float4x4};
        use rrising_render::{mesh, DrawCall, FrameUniforms, Instance, Renderer};
        use std::path::{Path, PathBuf};
        let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("VRISING_DATA"));
        let archives = list_archives(&data.join("StreamingAssets/ContentArchives")).unwrap();
        let content =
            GameContent::open(&data, &archives, Path::new("../../assets/typetrees.tpk")).unwrap();
        let identity = Instance::from_matrix(&float4x4::identity);
        let mut visual = SceneData {
            groups: vec![],
            markers: vec![],
            marker_entities: vec![],
            stats: SceneStats::default(),
            bounds: ([0.; 3], [0.; 3]),
        };
        attach_player_visual(&content, &mut visual, Entity::new(1, 1), identity).unwrap();
        let (_, samples) = player_walk_samples(&content, 4).unwrap();
        assert_eq!(samples.len(), visual.groups.len());
        let mut renderer = pollster::block_on(Renderer::new_offscreen(256, 256)).expect("GPU");
        let meshes: Vec<_> = visual
            .groups
            .iter()
            .zip(&samples)
            .map(|(group, frames)| {
                mesh::upload_vertices(
                    renderer.device(),
                    &group.name,
                    &frames[0],
                    &group.inds,
                    1,
                    true,
                )
                .unwrap()
            })
            .collect();
        let eye = float3::new(0., 0.85, 4.4);
        let center = float3::new(0., 0.85, 0.);
        let view = float4x4::scale(float3::new(1., 1., -1.))
            * float4x4::look_at(eye, center, float3::new(0., 1., 0.)).inverse();
        let proj = float4x4::perspective_fov(std::f32::consts::FRAC_PI_3, 1., 0.02, 40.);
        let uniforms = FrameUniforms {
            view_proj: Instance::from_matrix(&(rrising_render::clip_fix() * proj * view)).model,
            camera_pos: [eye.x, eye.y, eye.z, 0.],
            light_dir: [0.35, 0.85, 0.4, 0.85],
            ambient: [0.32, 0.36, 0.44, 0.],
        };
        let neutral = renderer.neutral_view();
        let instance = [identity];
        let draws: Vec<_> = meshes
            .iter()
            .map(|mesh| DrawCall {
                mesh,
                instances: &instance,
                texture: &neutral,
                cutout: false,
            })
            .collect();
        let first = renderer.render_to_image(&uniforms, &draws).unwrap().2;
        for (mesh, frames) in meshes.iter().zip(&samples) {
            mesh::update_vertices(renderer.queue(), mesh, &frames[1]).unwrap();
        }
        let second = renderer.render_to_image(&uniforms, &draws).unwrap().2;
        let differing = first
            .as_chunks::<4>()
            .0
            .iter()
            .zip(second.as_chunks::<4>().0.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(
            differing > 100,
            "only {differing} pixels changed between original walk samples"
        );
        println!("original walk samples: {differing} differing pixels from GPU vertex updates");
    }
}
