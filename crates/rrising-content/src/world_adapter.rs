//! Adapter from a decoded [`SceneFile`] to a [`rrising_ecs::World`].
//!
//! Every archetype and chunk of the scene is instantiated with its stored entity ids (index and
//! version preserved) and per-entity component bytes, using the recovered [`TypeLayouts`] to find
//! each component's slice of the serialized chunk. One scene becomes one world.
//!
//! What is and is not carried (see `docs/COMPATIBILITY.md`):
//! - Per-entity components, including managed 4-byte slots, are
//!   copied as raw serialized bytes. Managed slots are opaque: they are *not* valid managed-store
//!   handles and no managed object is attached.
//! - Used inline and external buffer elements are copied into owned ECS storage. Capacity and
//!   inline/external placement may be normalized. Known blob fields are relocated at world build.
//! - Entity references: the scene stores references in its own (scene-local) id space. A remap
//!   table (`rrising_ecs::EntityRemapTable`, source id -> live id, ids being preserved) is applied
//!   to every per-entity component's `Entity` fields (offsets from the recovered layouts); a
//!   reference that is not live in the scene becomes `Entity.Null`, as `RemapEntity` does. Buffer
//!   elements are remapped as well. Counts are in [`AdapterStats`].
//! - Shared indices group chunks, and serialized unmanaged shared bytes are retained separately.
//!   Those bytes are not live shared-component values: references and managed objects are not
//!   restored. Chunk-component values are not carried.
//! - The archetype/type order inside the store is the port's ("entity first, rest by packed
//!   value"), not necessarily the original's; component bytes are addressed by type, so this does
//!   not change what a query returns.
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use anyhow::{bail, ensure, Context, Result};
use rrising_ecs::{
    remap_elements, ArchetypeInput, ChunkInput, Entity, EntityIds, EntityRemapTable, RefCounts,
    SpecialTypes, TypeIndex, TypeManager, World, WorldInput,
};

use crate::entity_scenes::{SceneBlobAsset, SceneBlobAssets, SceneFile};
use crate::prefab_catalog::PrefabBlobPayload;
use crate::type_layouts::{EnabledSceneRows, TypeLayouts, ENTITY_HASH};

/// What the adapter copied or deliberately left out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AdapterStats {
    pub archetypes: usize,
    pub chunks: usize,
    pub entities: usize,
    /// Component arrays copied as raw bytes (all entities of a chunk counted once).
    pub component_arrays: usize,
    pub disabled_components: usize,
    /// Shared / chunk-component / zero-sized slots with no per-entity bytes to copy.
    pub skipped_value_less_slots: usize,
    /// References in per-entity components, classified before patching (then patched).
    pub scalar_refs: RefCounts,
    /// References in buffer elements (classified, not patched: no heap payload in the world).
    pub buffer_refs: RefCounts,
    /// The subset of `scalar_refs` that are `Parent.Value`.
    pub parent_refs: RefCounts,
    /// Dangling or stale-version references by component type name.
    pub unresolved_by_type: BTreeMap<String, usize>,
}

/// `Unity.Transforms.Parent` (checked by name in the acceptance test).
pub const PARENT_HASH: u64 = 0x6a6b_52ec_c95a_65e0;

/// A scene instantiated as a world, with the hash-to-type map needed to build queries.
#[derive(Debug)]
pub struct SceneWorld {
    pub world: World,
    pub type_of_hash: BTreeMap<u64, TypeIndex>,
    pub stats: AdapterStats,
    /// Scene-local serialized shared records, indexed by the chunk's nonzero shared index minus one.
    pub serialized_shared_values: Vec<SerializedSharedValue>,
    scene_blobs: Arc<BTreeMap<i32, PrefabBlobPayload>>,
}

/// Serialized scene data, not a live Unity managed object or a relocated native value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SerializedSharedValue {
    pub ty: TypeIndex,
    /// `None` denotes a managed record whose object is not decoded.
    pub unmanaged_bytes: Option<Vec<u8>>,
    /// Offline layout evidence says the value has no nonportable fields or unresolved references.
    pub portable: bool,
}

/// The source of a shared-component value for an entity in a scene world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneSharedValue<'a> {
    /// Serialized index zero. The original default value is type-dependent.
    Default,
    /// Raw serialized bytes; embedded references are not relocated.
    Unmanaged(&'a [u8]),
    /// The managed object has not been decoded.
    Managed,
}

impl SceneWorld {
    /// Address of a relocated serialized blob offset, valid for the lifetime of this world/store.
    pub fn scene_blob_address(&self, offset: i32) -> Option<u64> {
        if offset == -1 {
            Some(0)
        } else {
            self.scene_blobs
                .get(&offset)
                .map(PrefabBlobPayload::payload_address)
        }
    }

    /// Original blob payload for a relocated serialized offset.
    pub fn scene_blob_data(&self, offset: i32) -> Option<&[u8]> {
        self.scene_blobs
            .get(&offset)
            .map(|blob| blob.data.as_slice())
    }

    /// Inspect an entity's scene-local shared value without treating serialized pointers as live.
    pub fn serialized_shared_value(
        &self,
        entity: Entity,
        ty: TypeIndex,
    ) -> Result<SceneSharedValue<'_>> {
        ensure!(self.world.store.exists(entity)?, "entity is not live");
        let location = self
            .world
            .store
            .entity_in_chunk(entity.index)
            .context("missing entity location")?;
        let chunk = self
            .world
            .store
            .chunk(location.chunk)
            .context("missing entity chunk")?;
        let arch = self
            .world
            .store
            .archetypes()
            .get(usize::try_from(chunk.archetype_index)?)
            .context("missing archetype")?;
        let layout = &arch.archetype.data.layout;
        let shared_types = &arch.archetype.types
            [layout.first_shared_component as usize..layout.first_chunk_component as usize];
        let slot = shared_types
            .iter()
            .position(|t| t.type_index == ty)
            .context("entity lacks shared component type")?;
        let index = arch
            .chunk_data
            .get_shared_component_value(slot, usize::try_from(chunk.list_index)?)?;
        if index == 0 {
            return Ok(SceneSharedValue::Default);
        }
        let record = self
            .serialized_shared_values
            .get(usize::try_from(index - 1)?)
            .context("unknown serialized shared component index")?;
        ensure!(record.ty == ty, "serialized shared component type mismatch");
        Ok(match &record.unmanaged_bytes {
            Some(bytes) => SceneSharedValue::Unmanaged(bytes),
            None => SceneSharedValue::Managed,
        })
    }
}

/// Plain input plus the type registry it refers to.
#[derive(Debug)]
pub struct AdaptedScene {
    pub types: TypeManager,
    pub special: SpecialTypes,
    pub input: WorldInput,
    pub type_of_hash: BTreeMap<u64, TypeIndex>,
    pub stats: AdapterStats,
    pub serialized_shared_values: Vec<SerializedSharedValue>,
    disabled_components: Vec<(Entity, TypeIndex)>,
    buffer_payloads: Vec<(Entity, TypeIndex, Vec<u8>)>,
}

/// Original store initialization at RVA 0x8fa9840 populates these identities;
/// CreateArchetype (0x8fb0b30) uses them to classify Prefab/Disabled/meta chunks.
/// Only types present in this scene are registered. Absence is an unmatched slot,
/// not TypeIndex zero (which can alias another registered type).
fn special_types(types: &TypeManager, entity: TypeIndex) -> SpecialTypes {
    let get = |name| types.type_index_of(name).unwrap_or(SpecialTypes::UNSET);
    SpecialTypes {
        entity_type: entity,
        chunk_header: get("Unity.Entities.ChunkHeader"),
        prefab: get("Unity.Entities.Prefab"),
        spawn_tag: get("Unity.Entities.SpawnTag"),
        destroy_tag: get("Unity.Entities.DestroyTag"),
        cleanup_entity: get("Unity.Entities.CleanupEntity"),
        disabled: get("Unity.Entities.Disabled"),
        system_instance: get("Unity.Entities.SystemInstance"),
    }
}

/// Convert a scene into [`WorldInput`]. Refuses (errors) if any archetype lacks a recovered layout.
pub fn scene_to_world_input(layouts: &TypeLayouts, scene: &SceneFile) -> Result<AdaptedScene> {
    let resolved = layouts.resolve_scene(scene)?;
    ensure!(
        resolved.summary.unresolved_archetypes.is_empty(),
        "{} archetypes have no recovered layout; refusing a partial world",
        resolved.summary.unresolved_archetypes.len()
    );
    let mut types = TypeManager::default();
    types.register(rrising_ecs::TypeSpec::tag("<reserved null>"))?;
    let entity_spec = layouts
        .get(ENTITY_HASH)
        .context("layout report lacks Entity")?
        .spec
        .clone();
    let entity = types.register(entity_spec)?;
    let mut type_of_hash = BTreeMap::from([(ENTITY_HASH, entity)]);
    let mut archetypes = Vec::with_capacity(resolved.archetypes.len());
    let mut stats = AdapterStats::default();
    for (ai, arch) in resolved.archetypes.iter().enumerate() {
        let arch = arch.as_ref().context("unresolved archetype")?;
        let mut list = Vec::with_capacity(arch.hashes.len().saturating_sub(1));
        for (slot, &hash) in arch.hashes.iter().enumerate().skip(1) {
            let base = match type_of_hash.get(&hash) {
                Some(&t) => t,
                None => {
                    let spec = layouts
                        .get(hash)
                        .with_context(|| format!("archetype {ai}: hash {hash:016x} unknown"))?
                        .spec
                        .clone();
                    let t = types.register(spec)?;
                    type_of_hash.insert(hash, t);
                    t
                }
            };
            let mut t = base;
            if arch.types[slot].is_chunk_component() {
                t.value |=
                    (TypeIndex::K_CHUNK_COMPONENT_MASK | TypeIndex::K_ZERO_SIZED_MASK) as i32;
            }
            list.push(t);
        }
        archetypes.push(ArchetypeInput { types: list });
    }
    stats.archetypes = archetypes.len();

    // Pass 1: stored ids of every chunk; they become the remap table (source -> live id).
    let mut chunk_ids: Vec<Vec<Entity>> = Vec::with_capacity(resolved.chunks.len());
    let mut table = EntityRemapTable::new();
    for chunk in &resolved.chunks {
        let ai = usize::try_from(chunk.header.archetype_index)?;
        let arch = resolved.archetypes[ai].as_ref().context("archetype")?;
        let count = usize::try_from(chunk.header.count)?;
        let off = usize::try_from(arch.data.layout.offsets[0])?;
        let id_bytes = chunk
            .buffer
            .get(off..off + 8 * count)
            .with_context(|| format!("chunk {} entity slot outside buffer", chunk.index))?;
        let ids: Vec<Entity> = id_bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| {
                Entity::new(
                    i32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                    i32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                )
            })
            .collect();
        for &e in &ids {
            table.add(e, e);
        }
        chunk_ids.push(ids);
    }
    let shared = scene.shared_and_managed_components()?;
    let mut serialized_shared_values =
        Vec::with_capacity(shared.serialized_shared_component_count());
    for record in &shared.unmanaged {
        let ty = *type_of_hash
            .get(&record.stable_type_hash)
            .context("unknown unmanaged shared type")?;
        ensure!(
            ty.is_shared_component_type() && !ty.is_managed_type(),
            "invalid unmanaged shared type"
        );
        let recovered = layouts
            .get(record.stable_type_hash)
            .context("missing shared layout")?;
        ensure!(
            record.data.len() == usize::try_from(recovered.spec.type_size)?,
            "shared component size mismatch"
        );
        let portable = recovered.portable_value
            && recovered.entity_offsets.is_empty()
            && recovered.blob_offsets.as_ref().is_some_and(Vec::is_empty);
        serialized_shared_values.push(SerializedSharedValue {
            ty,
            unmanaged_bytes: Some(record.data.to_vec()),
            portable,
        });
    }
    for record in &shared.managed_shared_records {
        let ty = *type_of_hash
            .get(&record.stable_type_hash)
            .context("unknown managed shared type")?;
        ensure!(
            ty.is_shared_component_type() && ty.is_managed_type(),
            "invalid managed shared type"
        );
        serialized_shared_values.push(SerializedSharedValue {
            ty,
            unmanaged_bytes: None,
            portable: false,
        });
    }
    let mut shared_cursor = 0usize;
    let buffers = scene.buffer_data()?;
    let enabled = EnabledSceneRows::read(scene, &resolved)?;
    let mut disabled_components = Vec::new();
    let mut buffer_payloads = Vec::new();

    let mut chunks = Vec::with_capacity(resolved.chunks.len());
    for (ci, chunk) in resolved.chunks.iter().enumerate() {
        let ai = usize::try_from(chunk.header.archetype_index)?;
        let arch = resolved.archetypes[ai].as_ref().context("archetype")?;
        let count = usize::try_from(chunk.header.count)?;
        let layout = &arch.data.layout;
        let slice = |slot: usize| -> Result<&[u8]> {
            let off = usize::try_from(layout.offsets[slot])?;
            let stride = usize::from(layout.sizeofs[slot]);
            chunk
                .buffer
                .get(off..off + stride * count)
                .with_context(|| format!("chunk {} slot {slot} outside buffer", chunk.index))
        };
        let ids = std::mem::take(&mut chunk_ids[ci]);
        let mut shared_components = Vec::new();
        for &slot in arch.data.layout.memory_order_to_archetype.iter().skip(1) {
            let slot = usize::try_from(slot)?;
            let ty = arch.types[slot];
            if !ty.is_shared_component_type() || ty.is_chunk_component() {
                continue;
            }
            let value = *shared
                .shared_component_remap
                .get(shared_cursor)
                .context("shared component remap shorter than chunk slots")?;
            shared_cursor += 1;
            if value != 0 {
                let index = usize::try_from(value - 1)?;
                let hash = if index < shared.unmanaged.len() {
                    shared.unmanaged[index].stable_type_hash
                } else {
                    shared
                        .managed_shared_records
                        .get(index - shared.unmanaged.len())
                        .context("shared component index outside records")?
                        .stable_type_hash
                };
                ensure!(
                    hash == arch.hashes[slot],
                    "chunk {ci}: shared component type mismatch"
                );
            }
            shared_components.push((archetypes[ai].types[slot - 1], value));
        }
        let mut components = Vec::new();
        for slot in 1..arch.types.len() {
            let t = arch.types[slot];
            if t.is_enableable() && !t.is_chunk_component() {
                let memory_slot = usize::try_from(layout.archetype_to_memory_order[slot])?;
                for (row, &entity) in ids.iter().enumerate() {
                    if !enabled.get(ci, row, memory_slot, chunk.header.count)? {
                        disabled_components.push((entity, archetypes[ai].types[slot - 1]));
                    }
                }
            }
            if t.is_chunk_component() || t.is_shared_component_type() || t.is_zero_sized() {
                stats.skipped_value_less_slots += 1;
                continue;
            }
            if layout.sizeofs[slot] == 0 {
                bail!("slot {slot} of archetype {ai} has zero stride but is per-entity");
            }
            let hash = arch.hashes[slot];
            let rec = layouts.get(hash).context("recovered type")?;
            let stride = usize::from(layout.sizeofs[slot]);
            let mut bytes = slice(slot)?.to_vec();
            if t.is_buffer() {
                let elem = usize::try_from(rec.spec.element_size)?;
                ensure!(elem > 0, "buffer element size is zero");
                let patches = &buffers.per_chunk[chunk.index].patches;
                let base = usize::try_from(layout.offsets[slot])?;
                let mut c = RefCounts::default();
                for (i, &entity) in ids.iter().enumerate() {
                    let at = base + i * stride;
                    let h = chunk
                        .buffer
                        .get(at..at + 16)
                        .context("buffer header outside chunk")?;
                    let len = usize::try_from(i32::from_le_bytes(h[8..12].try_into()?))?;
                    let capacity = usize::try_from(i32::from_le_bytes(h[12..16].try_into()?))?;
                    ensure!(len <= capacity, "buffer length exceeds capacity");
                    let data = match patches
                        .iter()
                        .find(|p| usize::try_from(p.chunk_offset).ok() == Some(at))
                    {
                        Some(p) => p.data,
                        None => chunk
                            .buffer
                            .get(at + 16..at + stride)
                            .context("inline buffer outside chunk")?,
                    };
                    let used = len.checked_mul(elem).context("buffer size overflow")?;
                    let mut payload = data
                        .get(..used)
                        .context("buffer elements exceed data")?
                        .to_vec();
                    if !rec.entity_offsets.is_empty() {
                        remap_elements(
                            &mut payload,
                            elem,
                            &rec.entity_offsets,
                            &table,
                            true,
                            &mut c,
                        );
                    }
                    buffer_payloads.push((entity, archetypes[ai].types[slot - 1], payload));
                }
                stats.buffer_refs = add_counts(stats.buffer_refs, c);
                let bad = c.dangling + c.stale_version;
                if bad > 0 {
                    *stats
                        .unresolved_by_type
                        .entry(rec.name.clone())
                        .or_default() += bad;
                }
            } else if !rec.entity_offsets.is_empty() && !t.is_managed_component() {
                let mut c = RefCounts::default();
                remap_elements(
                    &mut bytes,
                    stride,
                    &rec.entity_offsets,
                    &table,
                    true,
                    &mut c,
                );
                stats.scalar_refs = add_counts(stats.scalar_refs, c);
                if hash == PARENT_HASH {
                    stats.parent_refs = add_counts(stats.parent_refs, c);
                }
                let bad = c.dangling + c.stale_version;
                if bad > 0 {
                    *stats
                        .unresolved_by_type
                        .entry(rec.name.clone())
                        .or_default() += bad;
                }
            }
            components.push((archetypes[ai].types[slot - 1], bytes));
            stats.component_arrays += 1;
        }
        stats.chunks += 1;
        stats.entities += count;
        chunks.push(ChunkInput {
            shared_components,
            archetype: ai,
            entities: EntityIds::Preserved(ids),
            components,
        });
    }
    ensure!(
        shared_cursor == shared.shared_component_remap.len(),
        "unused shared component remap entries"
    );
    stats.disabled_components = disabled_components.len();
    Ok(AdaptedScene {
        special: special_types(&types, entity),
        types,
        input: WorldInput { archetypes, chunks },
        type_of_hash,
        stats,
        serialized_shared_values,
        disabled_components,
        buffer_payloads,
    })
}

fn index_scene_blobs<'a>(blobs: &SceneBlobAssets<'a>) -> Result<HashMap<i32, SceneBlobAsset<'a>>> {
    let mut index = HashMap::with_capacity(blobs.blobs.len());
    for &blob in &blobs.blobs {
        let offset =
            i32::try_from(blob.offset).context("scene blob offset exceeds serialized range")?;
        ensure!(
            index.insert(offset, blob).is_none(),
            "duplicate scene blob offset"
        );
    }
    Ok(index)
}

fn patch_scene_blob_fields(
    bytes: &mut [u8],
    stride: usize,
    offsets: &[i32],
    blobs: &HashMap<i32, SceneBlobAsset<'_>>,
    owned: &mut BTreeMap<i32, PrefabBlobPayload>,
) -> Result<()> {
    if offsets.is_empty() {
        return Ok(());
    }
    ensure!(
        stride > 0 && bytes.len().is_multiple_of(stride),
        "invalid blob component stride"
    );
    for element in bytes.chunks_exact_mut(stride) {
        for &offset in offsets {
            let at = usize::try_from(offset)?;
            let field = element
                .get_mut(at..at.checked_add(8).context("blob field overflow")?)
                .context("blob field outside component")?;
            let serialized = i64::from_le_bytes(field.try_into()?);
            let serialized = i32::try_from(serialized).context("invalid serialized blob offset")?;
            let address = if serialized == -1 {
                0
            } else {
                if let std::collections::btree_map::Entry::Vacant(entry) = owned.entry(serialized) {
                    let source = blobs
                        .get(&serialized)
                        .context("unknown scene blob offset")?;
                    entry.insert(PrefabBlobPayload::new(source.hash, source.data, &[])?);
                }
                owned[&serialized].payload_address()
            };
            field.copy_from_slice(&address.to_le_bytes());
        }
    }
    Ok(())
}

/// Convert and build the world for one scene.
pub fn build_scene_world(layouts: &TypeLayouts, scene: &SceneFile) -> Result<SceneWorld> {
    let mut a = scene_to_world_input(layouts, scene)?;
    let blobs = scene.blob_assets()?;
    let blob_index = index_scene_blobs(&blobs)?;
    let mut owned_blobs = BTreeMap::new();
    let by_type: BTreeMap<_, _> = a
        .type_of_hash
        .iter()
        .map(|(&hash, &ty)| (ty, hash))
        .collect();
    for chunk in &mut a.input.chunks {
        for (ty, bytes) in &mut chunk.components {
            if ty.is_buffer() || ty.is_managed_component() {
                continue;
            }
            let Some(layout) = by_type.get(ty).and_then(|h| layouts.get(*h)) else {
                continue;
            };
            if let Some(offsets) = &layout.blob_offsets {
                let stride = usize::try_from(layout.spec.size_in_chunk)?;
                patch_scene_blob_fields(bytes, stride, offsets, &blob_index, &mut owned_blobs)?;
            }
        }
    }
    for (_, ty, bytes) in &mut a.buffer_payloads {
        let Some(layout) = by_type.get(ty).and_then(|h| layouts.get(*h)) else {
            continue;
        };
        if let Some(offsets) = &layout.blob_offsets {
            patch_scene_blob_fields(
                bytes,
                usize::try_from(layout.spec.element_size)?,
                offsets,
                &blob_index,
                &mut owned_blobs,
            )?;
        }
    }
    let owned_blobs = Arc::new(owned_blobs);
    let mut world = World::build_with(
        a.types,
        a.special,
        i32::try_from(a.stats.entities + 2)?,
        &a.input,
        true,
    )?;
    let blob_ids: Vec<_> = owned_blobs
        .iter()
        .map(|(&offset, blob)| (blob.payload_address(), offset as u64 + 1))
        .collect();
    for (&hash, &ty) in &a.type_of_hash {
        if let Some(offsets) = layouts
            .get(hash)
            .and_then(|layout| layout.blob_offsets.as_ref())
        {
            if !offsets.is_empty() && !ty.is_shared_component_type() && !ty.is_managed_component() {
                world.register_blob_fingerprint(
                    ty,
                    offsets
                        .iter()
                        .map(|&offset| usize::try_from(offset))
                        .collect::<std::result::Result<Vec<_>, _>>()?,
                    &blob_ids,
                )?;
            }
        }
    }
    world.store.retain_lifetime(owned_blobs.clone());
    for (entity, ty, payload) in &a.buffer_payloads {
        world
            .store
            .set_buffer_payload(&world.types, *entity, *ty, payload)?;
    }
    for &(entity, ty) in &a.disabled_components {
        world.store.set_component_enabled(entity, ty, false)?;
    }
    Ok(SceneWorld {
        world,
        type_of_hash: a.type_of_hash,
        stats: a.stats,
        serialized_shared_values: a.serialized_shared_values,
        scene_blobs: owned_blobs,
    })
}

fn add_counts(a: RefCounts, b: RefCounts) -> RefCounts {
    RefCounts {
        null: a.null + b.null,
        live: a.live + b.live,
        stale_version: a.stale_version + b.stale_version,
        dangling: a.dangling + b.dangling,
    }
}

#[cfg(test)]
mod special_type_tests {
    use super::*;
    use rrising_ecs::{EntityQuery, TypeCategory, TypeSpec};

    #[test]
    fn indexed_blobs_patch_null_and_share_repeated_references() -> Result<()> {
        use crate::entity_scenes::{BlobAssetBatchHeader, SceneBlobAsset, SceneBlobAssets};
        let blob = SceneBlobAsset {
            index: 0,
            offset: 64,
            hash: 42,
            length: 4,
            data: &[1, 2, 3, 4],
        };
        let scene = SceneBlobAssets {
            header: BlobAssetBatchHeader::default(),
            blobs: vec![blob],
        };
        let index = index_scene_blobs(&scene)?;
        let mut owned = BTreeMap::new();
        let mut fields = [
            64i64.to_le_bytes(),
            (-1i64).to_le_bytes(),
            64i64.to_le_bytes(),
        ]
        .concat();
        patch_scene_blob_fields(&mut fields, 8, &[0], &index, &mut owned)?;
        let read = |at| u64::from_le_bytes(fields[at..at + 8].try_into().unwrap());
        assert_eq!(read(0), read(16));
        assert_ne!(read(0), 0);
        assert_eq!(read(8), 0);
        assert_eq!(owned.len(), 1);
        let mut missing = 65i64.to_le_bytes();
        assert!(patch_scene_blob_fields(&mut missing, 8, &[0], &index, &mut owned).is_err());
        let duplicate = SceneBlobAssets {
            header: BlobAssetBatchHeader::default(),
            blobs: vec![blob, blob],
        };
        assert!(index_scene_blobs(&duplicate).is_err());
        Ok(())
    }

    #[test]
    fn scene_registry_special_identities_control_the_actual_world_query() {
        let mut types = TypeManager::default();
        let mut spec = TypeSpec::component_data("Unity.Entities.Entity", 8, 8);
        spec.category = TypeCategory::EntityData;
        let entity = types.register(spec).unwrap();
        let position = types
            .register(TypeSpec::component_data("Position", 12, 4))
            .unwrap();
        let prefab = types
            .register(TypeSpec::tag("Unity.Entities.Prefab"))
            .unwrap();
        let disabled = types
            .register(TypeSpec::tag("Unity.Entities.Disabled"))
            .unwrap();
        let special = special_types(&types, entity);
        assert_eq!(special.chunk_header, SpecialTypes::UNSET);
        assert_eq!(special.system_instance, SpecialTypes::UNSET);
        let input = WorldInput {
            archetypes: vec![
                ArchetypeInput {
                    types: vec![position],
                },
                ArchetypeInput {
                    types: vec![position, prefab],
                },
                ArchetypeInput {
                    types: vec![position, disabled],
                },
            ],
            chunks: (0..3)
                .map(|archetype| ChunkInput {
                    shared_components: vec![],
                    archetype,
                    entities: EntityIds::Fresh(1),
                    components: vec![],
                })
                .collect(),
        };
        let world = World::build(types, special, 8, &input).unwrap();
        let mut q = EntityQuery {
            all: vec![position],
            ..Default::default()
        };
        assert_eq!(world.query_count(&q), 1);
        q.include_prefab = true;
        assert_eq!(world.query_count(&q), 2);
        q.include_prefab = false;
        q.include_disabled = true;
        assert_eq!(world.query_count(&q), 2);
        q.include_prefab = true;
        assert_eq!(world.query_count(&q), 3);
    }
}
