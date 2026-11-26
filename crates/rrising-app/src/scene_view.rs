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
const STEP_NS: u64 = 16_666_667;

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
    /// Advance a retained world on fixed steps; the step cap limits catch-up after a pause.
    pub fn advance(&mut self, elapsed_ns: u64, max_steps: u32) -> Result<u32> {
        self.schedule
            .advance(&mut self.world, elapsed_ns, max_steps)
    }

    fn instance(&self, e: Entity) -> Option<Instance> {
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

#[cfg(test)]
mod managed_array_tests {
    use super::{needs_runtime_or_range_resolution, parse_index_arrays};
    use rrising_ecs::MaterialMeshInfo;

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
}
