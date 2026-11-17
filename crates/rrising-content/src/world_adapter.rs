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
//! - Shared-component values, chunk-component values and entity-reference remapping are not
//!   carried. Their archetypes are admitted through the store's opaque mode, which keeps the
//!   original layout algorithm but not shared-value chunk grouping: the store packs entities into
//!   chunks itself, so chunk boundaries can differ from the scene's.
//! - The archetype/type order inside the store is the port's ("entity first, rest by packed
//!   value"), not necessarily the original's; component bytes are addressed by type, so this does
//!   not change what a query returns.
use std::collections::BTreeMap;

use anyhow::{bail, ensure, Context, Result};
use rrising_ecs::{
    ArchetypeInput, ChunkInput, Entity, EntityIds, SpecialTypes, TypeIndex, TypeManager, World,
    WorldInput,
};

use crate::entity_scenes::SceneFile;
use crate::type_layouts::{TypeLayouts, ENTITY_HASH};

/// What the adapter copied or deliberately left out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AdapterStats {
    pub archetypes: usize,
    pub chunks: usize,
    pub entities: usize,
    /// Component arrays copied as raw bytes (all entities of a chunk counted once).
    pub component_arrays: usize,
    /// Shared / chunk-component / zero-sized slots with no per-entity bytes to copy.
    pub skipped_value_less_slots: usize,
}

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

    let mut chunks = Vec::with_capacity(resolved.chunks.len());
    for chunk in &resolved.chunks {
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
        let id_bytes = slice(0)?;
        ensure!(id_bytes.len() == count * 8, "Entity slot is not 8 bytes");
        let ids = (0..count)
            .map(|i| {
                let r = |o: usize| -> Result<i32> {
                    Ok(i32::from_le_bytes(
                        id_bytes[i * 8 + o..i * 8 + o + 4].try_into()?,
                    ))
                };
                Ok(Entity::new(r(0)?, r(4)?))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut components = Vec::new();
        for slot in 1..arch.types.len() {
            let t = arch.types[slot];
            if t.is_chunk_component() || t.is_shared_component_type() || t.is_zero_sized() {
                stats.skipped_value_less_slots += 1;
                continue;
            }
            if layout.sizeofs[slot] == 0 {
                bail!("slot {slot} of archetype {ai} has zero stride but is per-entity");
            }
            components.push((archetypes[ai].types[slot - 1], slice(slot)?.to_vec()));
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
    Ok(AdaptedScene {
        special: SpecialTypes::new(entity),
        types,
        input: WorldInput { archetypes, chunks },
        type_of_hash,
        stats,
    })
}

/// Convert and build the world for one scene.
pub fn build_scene_world(layouts: &TypeLayouts, scene: &SceneFile) -> Result<SceneWorld> {
    let a = scene_to_world_input(layouts, scene)?;
    let world = World::build_with(
        a.types,
        a.special,
        i32::try_from(a.stats.entities + 2)?,
        &a.input,
        true,
    )?;
    Ok(SceneWorld {
        world,
        type_of_hash: a.type_of_hash,
        stats: a.stats,
    })
}
