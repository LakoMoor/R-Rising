//! Adapter from a decoded [`SceneFile`] to a [`rrising_ecs::World`].
//!
//! Every archetype and chunk of the scene is instantiated with its stored entity ids (index and
//! version preserved) and per-entity component bytes, using the recovered [`TypeLayouts`] to find
//! each component's slice of the serialized chunk. One scene becomes one world.
//!
//! What is and is not carried (see `docs/COMPATIBILITY.md`):
//! - Per-entity components, including zero-filled buffer headers and managed 4-byte slots, are
//!   copied as raw serialized bytes. Managed slots are opaque: they are *not* valid managed-store
//!   handles and no managed object is attached.
//! - Buffer headers carry length/capacity but there is no heap payload behind them.
//! - Entity references: the scene stores references in its own (scene-local) id space. A remap
//!   table (`rrising_ecs::EntityRemapTable`, source id -> live id, ids being preserved) is applied
//!   to every per-entity component's `Entity` fields (offsets from the recovered layouts); a
//!   reference that is not live in the scene becomes `Entity.Null`, as `RemapEntity` does. Buffer
//!   elements have no heap payload in the `World`, so their references are classified from the
//!   scene's buffer data but not patched. Counts are in [`AdapterStats`].
//! - Shared-component values and chunk-component values are not carried. Their archetypes are admitted through the store's opaque mode, which keeps the
//!   original layout algorithm but not shared-value chunk grouping: the store packs entities into
//!   chunks itself, so chunk boundaries can differ from the scene's.
//! - The archetype/type order inside the store is the port's ("entity first, rest by packed
//!   value"), not necessarily the original's; component bytes are addressed by type, so this does
//!   not change what a query returns.
use std::collections::BTreeMap;

use anyhow::{bail, ensure, Context, Result};
use rrising_ecs::{
    classify_elements, remap_elements, ArchetypeInput, ChunkInput, Entity, EntityIds,
    EntityRemapTable, RefCounts, SpecialTypes, TypeIndex, TypeManager, World, WorldInput,
};

use crate::entity_scenes::SceneFile;
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
}

/// Plain input plus the type registry it refers to.
#[derive(Debug)]
pub struct AdaptedScene {
    pub types: TypeManager,
    pub special: SpecialTypes,
    pub input: WorldInput,
    pub type_of_hash: BTreeMap<u64, TypeIndex>,
    pub stats: AdapterStats,
    disabled_components: Vec<(Entity, TypeIndex)>,
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
    let buffers = scene.buffer_data()?;
    let enabled = EnabledSceneRows::read(scene, &resolved)?;
    let mut disabled_components = Vec::new();

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
            if !rec.entity_offsets.is_empty() && !t.is_managed_component() {
                let mut c = RefCounts::default();
                if t.is_buffer() {
                    let elem = usize::try_from(rec.spec.element_size)?;
                    let patches = &buffers.per_chunk[chunk.index].patches;
                    let base = usize::try_from(layout.offsets[slot])?;
                    for i in 0..count {
                        let at = base + i * stride;
                        let h = &chunk.buffer[at..at + 16];
                        let len = usize::try_from(i32::from_le_bytes(h[8..12].try_into()?))?;
                        let data: &[u8] = match patches
                            .iter()
                            .find(|p| usize::try_from(p.chunk_offset).ok() == Some(at))
                        {
                            Some(p) => p.data,
                            None => &chunk.buffer[at + 16..at + stride],
                        };
                        let used = data.get(..len * elem).with_context(|| {
                            format!(
                                "chunk {} buffer {at}: {len} elements exceed data",
                                chunk.index
                            )
                        })?;
                        classify_elements(used, elem, &rec.entity_offsets, &table, &mut c);
                    }
                    stats.buffer_refs = add_counts(stats.buffer_refs, c);
                } else {
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
            archetype: ai,
            entities: EntityIds::Preserved(ids),
            components,
        });
    }
    stats.disabled_components = disabled_components.len();
    Ok(AdaptedScene {
        special: special_types(&types, entity),
        types,
        input: WorldInput { archetypes, chunks },
        type_of_hash,
        stats,
        disabled_components,
    })
}

/// Convert and build the world for one scene.
pub fn build_scene_world(layouts: &TypeLayouts, scene: &SceneFile) -> Result<SceneWorld> {
    let a = scene_to_world_input(layouts, scene)?;
    let mut world = World::build_with(
        a.types,
        a.special,
        i32::try_from(a.stats.entities + 2)?,
        &a.input,
        true,
    )?;
    for &(entity, ty) in &a.disabled_components {
        world.store.set_component_enabled(entity, ty, false)?;
    }
    Ok(SceneWorld {
        world,
        type_of_hash: a.type_of_hash,
        stats: a.stats,
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
