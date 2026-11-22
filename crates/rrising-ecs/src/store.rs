//! `EntityComponentStore`: entity allocation/free with version checks, create/destroy inside
//! archetype chunks, and a minimal chunk-iterating query.
//!
//! Every `src:` address is an RVA in the pinned GameAssembly (decompiled bodies live in the
//! git-ignored `evidence/decompiled/Unity.Entities.dll.c`; nothing from it is copied here).
//!
//! # Entity table (original field offsets in `EntityComponentStore`)
//! - `int* m_VersionByEntity` `+0x0`, `EntityInChunk* m_EntityInChunkByEntity` `+0x8`
//!   (`{ChunkIndex Chunk; int IndexInChunk}`, 8 bytes), `int m_NextFreeEntityIndex` `+0x190`,
//!   `int m_EntityOrderVersion` `+0x194`, `uint m_GlobalSystemVersion` `+0x198`,
//!   `int m_EntitiesCapacity` `+0x19c`, `long m_NextChunkSequenceNumber` `+0x188`,
//!   `int* m_ComponentTypeOrderVersion` `+0x10` (0x4000 ints).
//! - A slot is **live** iff `Version[index] == entity.Version` and `Chunk != 0`
//!   (`Exists` @ 0x8fabb00). A free slot has `Chunk == 0` and `IndexInChunk` = next free index,
//!   `-1` terminating the list (`InitializeAdditionalCapacity` @ 0x8fa96e0 chains `i -> i+1`,
//!   version 1, last slot `-1`).
//! - `AllocateEntities` @ (`_174`, RVA 0x8fb8xxx; the dump names it `AllocateEntities_174`): pop the head; if its link is `-1` call
//!   `IncreaseCapacity` (@ 0x8fa9050, doubles, re-initialises from the old *last* slot and
//!   resets its version to 1 - kept; in practice benign because that slot is only the head with a
//!   `-1` link while it has never been handed out, so growth fires as the last slot is allocated); the entity gets the slot's
//!   current version; the slot records `{chunk, start + i}`.
//! - `DeallocateDataEntitiesInChunk` (`_175` @ 0x8fb8f70): walks the batch **backwards**, zeroes
//!   `Chunk`, `Version++`, links each slot to the previous head, so afterwards the head is the
//!   batch's first entity. `m_EntityOrderVersion++` once per batch.
//! - `DestroyEntities` (`_203`): groups the input into runs that are contiguous in one chunk and
//!   skips stale handles (version mismatch) instead of failing.
//! - `ChunkDataUtility.Deallocate_49` @ 0x902d890, `SetChunkCount_39` @ 0x902bc70,
//!   `AddEmptyChunk_57` @ 0x902f060, `ReleaseChunk_37` @ 0x902b8a0, and the non-shared branch of
//!   `Archetype.EmptySlotTracking*` (@ 0x911caa0 / 0x911c9e0 / 0x911cb60) define the chunk
//!   bookkeeping.
//!
//! # Not ported (explicit limits)
//! Archetypes with shared, chunk, managed or cleanup components are refused: they need
//! `ChunkListMap`, meta-chunk entities, managed deferred commands and the cleanup archetype
//! (`CreateMetaEntityForChunk`, `DestroyMetaChunkEntity`, `AssertEntityNotProtectedFromDestruction`).
//! Also absent: cross-archetype enable-bit moves, entity names, query
//! registration (`RegisterInQueries`), `EntityPatchInfo` rewriting, `InitializeComponents` defaults
//! (new component bytes are zeroed). The query matcher is a port-side minimum, see [`EntityQuery`].

use std::collections::HashMap;

use anyhow::{bail, ensure, Context, Result};

use crate::archetype::{create_archetype_data, Archetype, ArchetypeFlags, SpecialTypes};
use crate::chunk::{Chunk, ChunkIndex};
use crate::entity::Entity;
use crate::type_table::{
    AccessMode, ComponentType, ComponentTypeInArchetype, TypeIndex, TypeManager,
};

/// `ThrowIfEntitiesPerWorldIsTooHigh` @ 0x8fa9500: `0x8000000 < requested` throws.
pub const K_MAX_ENTITIES_PER_WORLD: i64 = 0x800_0000;
/// `m_ComponentTypeOrderVersion` holds `TypeIndex & 0x3fff` slots: 0x4000.
const K_ORDER_VERSION_SLOTS: usize = 0x4000;

/// `EntityComponentStore.EntityInChunk` (`Chunk` at +0, `IndexInChunk` at +4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntityInChunk {
    pub chunk: ChunkIndex,
    pub index_in_chunk: i32,
}

impl Default for ChunkIndex {
    fn default() -> Self {
        ChunkIndex::NULL
    }
}

/// An archetype plus the chunk list data the original keeps inside `Archetype.Chunks`.
#[derive(Clone, Debug)]
pub struct StoredArchetype {
    pub archetype: Archetype,
    pub chunk_data: crate::chunk_data::ArchetypeChunkData,
    /// `Archetype.ChunksWithEmptySlots` (+0x18) for archetypes without shared components: chunk
    /// indices, each chunk remembering its slot in `Chunk::list_with_empty_slots_index`. (The older
    /// `Archetype::chunks_with_empty_slots` field is not used by the store.)
    pub chunks_with_empty_slots: Vec<i32>,
}

/// Port-side query description: types with `AccessMode::Exclude` must be absent, every other type
/// must be present. The default exclusion of `Disabled`/`Prefab` archetypes is checked against the
/// binary: `EntityQueryManager.IsMatchingArchetype` (@ 0x90f46c0) rejects an archetype whose
/// `Flags` (+0x122) has bit 4 (`Disabled`) unless query options bit 2 is set, and bit 8 (`Prefab`)
/// unless options bit 1 is set. Not ported from that function: the other option gates (chunk
/// header, system instance, spawn/destroy tag), the bloom-filter pre-check, any/none ordering and
/// version checks; verified by reading the decompiled body, not by native replay.
#[derive(Clone, Debug, Default)]
pub struct EntityQuery {
    pub all: Vec<TypeIndex>,
    pub none: Vec<TypeIndex>,
    pub include_disabled: bool,
    pub include_prefab: bool,
}

impl EntityQuery {
    pub fn new(types: &[ComponentType]) -> Self {
        let mut q = Self::default();
        for t in types {
            if t.access_mode_type == AccessMode::Exclude {
                q.none.push(t.type_index);
            } else {
                q.all.push(t.type_index);
            }
        }
        q
    }

    pub fn matches(&self, archetype: &Archetype) -> bool {
        let has = |t: &TypeIndex| archetype.index_of_type(*t).is_some();
        if !self.all.iter().all(has) || self.none.iter().any(|ty| !ty.is_enableable() && has(ty)) {
            return false;
        }
        let f = archetype.flags();
        if f.contains(ArchetypeFlags::DISABLED) && !self.include_disabled {
            return false;
        }
        if f.contains(ArchetypeFlags::PREFAB) && !self.include_prefab {
            return false;
        }
        true
    }
}

fn mask_range(start: usize, end: usize) -> u128 {
    (u128::MAX >> (128 - (end - start))) << start
}

fn unpack_mask(words: [u64; 2]) -> u128 {
    u128::from(words[0]) | (u128::from(words[1]) << 64)
}

fn pack_mask(mask: u128) -> [u64; 2] {
    [mask as u64, (mask >> 64) as u64]
}

struct LifetimeOwner(Box<dyn std::any::Any + Send + Sync>);

impl std::fmt::Debug for LifetimeOwner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("LifetimeOwner")
            .field(&self.0.type_id())
            .finish()
    }
}

#[derive(Debug)]
pub struct EntityComponentStore {
    versions: Vec<i32>,
    entity_in_chunk: Vec<EntityInChunk>,
    next_free_entity_index: i32,
    entity_order_version: i32,
    global_system_version: u32,
    capacity: i32,
    component_type_order_version: Vec<i32>,
    next_chunk_sequence_number: u64,
    chunks: Vec<Option<Chunk>>,
    free_chunk_slots: Vec<i32>,
    archetypes: Vec<StoredArchetype>,
    by_types: HashMap<Vec<i32>, usize>,
    external_buffers: HashMap<i32, HashMap<TypeIndex, Vec<u8>>>,
    lifetime_owners: Vec<LifetimeOwner>,
    special: SpecialTypes,
    admit_opaque_special_components: bool,
}

impl EntityComponentStore {
    /// `EntityComponentStore.Create` @ 0x8fa9840: capacity arrays, `GlobalSystemVersion = 1`,
    /// free list chained by `InitializeAdditionalCapacity(0)`.
    pub fn new(initial_capacity: i32, special: SpecialTypes) -> Result<Self> {
        ensure!(initial_capacity >= 1, "entity capacity must be at least 1");
        ensure!(
            special.entity_type_is_set(),
            "SpecialTypes.entity_type must be set (use SpecialTypes::new)"
        );
        ensure!(
            (initial_capacity as i64) <= K_MAX_ENTITIES_PER_WORLD,
            "entities per world too high"
        );
        let mut s = Self {
            versions: Vec::new(),
            entity_in_chunk: Vec::new(),
            next_free_entity_index: 0,
            entity_order_version: 0,
            global_system_version: 1,
            capacity: 0,
            component_type_order_version: vec![0; K_ORDER_VERSION_SLOTS],
            next_chunk_sequence_number: 0,
            // index 0 is ChunkIndex.Null
            chunks: vec![None],
            free_chunk_slots: Vec::new(),
            archetypes: Vec::new(),
            by_types: HashMap::new(),
            external_buffers: HashMap::new(),
            lifetime_owners: Vec::new(),
            special,
            admit_opaque_special_components: false,
        };
        s.resize_and_initialize(0, initial_capacity)?;
        Ok(s)
    }

    /// `ResizeUnmanagedArrays` + capacity store + `InitializeAdditionalCapacity(start)`.
    fn resize_and_initialize(&mut self, start: i32, new_capacity: i32) -> Result<()> {
        ensure!(
            (new_capacity as i64) <= K_MAX_ENTITIES_PER_WORLD,
            "entities per world too high: {new_capacity} > {K_MAX_ENTITIES_PER_WORLD}"
        );
        self.versions.resize(new_capacity as usize, 0);
        self.entity_in_chunk
            .resize(new_capacity as usize, EntityInChunk::default());
        self.capacity = new_capacity;
        for i in start..new_capacity {
            let slot = &mut self.entity_in_chunk[i as usize];
            slot.index_in_chunk = i + 1;
            slot.chunk = ChunkIndex::NULL;
            self.versions[i as usize] = 1;
        }
        self.entity_in_chunk[(new_capacity - 1) as usize].index_in_chunk = -1;
        Ok(())
    }

    /// `IncreaseCapacity` @ 0x8fa9050: doubles; re-initialises from the old last slot.
    fn increase_capacity(&mut self) -> Result<()> {
        let new_cap = self.capacity as i64 * 2;
        ensure!(
            new_cap <= K_MAX_ENTITIES_PER_WORLD,
            "entities per world too high: {new_cap} > {K_MAX_ENTITIES_PER_WORLD}"
        );
        let start = if self.capacity > 0 {
            self.capacity - 1
        } else {
            0
        };
        self.resize_and_initialize(start, new_cap as i32)
    }

    /// Admit archetypes with shared/managed/chunk/cleanup components as *opaque*: the archetype
    /// layout (offsets, capacity, per-entity slots) is the original algorithm's, but shared values,
    /// chunk-component storage (meta chunks), managed objects and the cleanup archetype are not
    /// modelled; entities with different shared values may share a chunk. Off by default.
    pub fn set_admit_opaque_special_components(&mut self, admit: bool) {
        self.admit_opaque_special_components = admit;
    }

    /// Keeps backing storage alive for addresses held by component values.
    pub fn retain_lifetime<T: Send + Sync + 'static>(&mut self, owner: T) {
        self.lifetime_owners.push(LifetimeOwner(Box::new(owner)));
    }

    pub fn capacity(&self) -> i32 {
        self.capacity
    }
    pub fn next_free_entity_index(&self) -> i32 {
        self.next_free_entity_index
    }
    pub fn entity_order_version(&self) -> i32 {
        self.entity_order_version
    }
    pub fn global_system_version(&self) -> u32 {
        self.global_system_version
    }
    pub fn component_type_order_version(&self, t: TypeIndex) -> i32 {
        self.component_type_order_version[(t.value as u32 & TypeIndex::K_INDEX_MASK) as usize]
    }
    pub fn archetypes(&self) -> &[StoredArchetype] {
        &self.archetypes
    }
    pub fn chunk(&self, c: ChunkIndex) -> Option<&Chunk> {
        self.chunks.get(c.value as usize).and_then(|c| c.as_ref())
    }
    pub fn entity_in_chunk(&self, index: i32) -> Option<EntityInChunk> {
        self.entity_in_chunk.get(index as usize).copied()
    }

    /// `GetEntityByEntityIndex` @ 0x8fa96b0: `(version, index)` or `Entity.Null` when out of range.
    pub fn get_entity_by_entity_index(&self, index: i32) -> Entity {
        if index >= 0 && index < self.capacity {
            Entity::new(index, self.versions[index as usize])
        } else {
            Entity::NULL
        }
    }

    /// `ValidateEntity` @ 0x8fbfb40: negative or `>= capacity` indices throw.
    fn validate(&self, e: Entity) -> Result<()> {
        ensure!(e.index >= 0, "entity index {} is negative", e.index);
        ensure!(
            e.index < self.capacity,
            "entity index {} outside capacity {}",
            e.index,
            self.capacity
        );
        Ok(())
    }

    /// `EntityComponentStore.Exists` @ 0x8fabb00: validates the index (error), then
    /// `Version[index] == e.Version && EntityInChunk[index].Chunk != 0`.
    pub fn exists(&self, e: Entity) -> Result<bool> {
        self.validate(e)?;
        let i = e.index as usize;
        Ok(self.versions[i] == e.version && !self.entity_in_chunk[i].chunk.is_null())
    }

    // ---- archetypes -------------------------------------------------------------------------

    /// Find or build the archetype for a type set. Entity is always archetype type 0 (the port's
    /// ordering rule: entity first, then the rest sorted by packed value; the original's
    /// `GetOrCreateArchetype` ordering was not re-derived here).
    pub fn get_or_create_archetype(
        &mut self,
        tm: &TypeManager,
        types: &[TypeIndex],
    ) -> Result<usize> {
        let entity = self.special.entity_type;
        let mut rest: Vec<TypeIndex> = types.iter().copied().filter(|t| *t != entity).collect();
        rest.sort();
        rest.dedup();
        let mut list = vec![entity];
        list.extend(rest);
        let key: Vec<i32> = list.iter().map(|t| t.value).collect();
        if let Some(&i) = self.by_types.get(&key) {
            return Ok(i);
        }
        let data = create_archetype_data(&list, tm.infos(), &self.special)?;
        let in_arch: Vec<ComponentTypeInArchetype> = list
            .iter()
            .copied()
            .map(ComponentTypeInArchetype::new)
            .collect();
        let idx = self.archetypes.len();
        let archetype = Archetype::new(idx as i32, in_arch, data);
        let l = archetype.layout();
        let special_components = l.num_shared_components() > 0
            || l.num_managed_components() > 0
            || l.num_chunk_components(archetype.types_count) > 0
            || archetype.types.iter().any(|t| t.is_cleanup_component());
        if special_components && !self.admit_opaque_special_components {
            bail!("archetypes with shared/managed/chunk/cleanup components are not supported yet");
        }
        ensure!(l.chunk_capacity > 0, "archetype has zero chunk capacity");
        ensure!(
            archetype.enableable_types_count == 0 || l.chunk_capacity <= 128,
            "enableable archetype exceeds 128-row mask capacity"
        );
        let chunk_data = crate::chunk_data::ArchetypeChunkData::new(
            archetype.types_count as usize,
            l.num_shared_components() as usize,
        )?;
        self.archetypes.push(StoredArchetype {
            archetype,
            chunk_data,
            chunks_with_empty_slots: Vec::new(),
        });
        self.by_types.insert(key, idx);
        Ok(idx)
    }

    // ---- chunk bookkeeping ------------------------------------------------------------------

    fn chunk_mut(&mut self, c: ChunkIndex) -> Result<&mut Chunk> {
        self.chunks
            .get_mut(c.value as usize)
            .and_then(|c| c.as_mut())
            .ok_or_else(|| anyhow::anyhow!("chunk {} is not allocated", c.value))
    }

    /// `AllocateChunk` @ 0x8fb... (`_110`) + `AddEmptyChunk_57` + `AddToChunkList_28`.
    fn add_empty_chunk(&mut self, arch: usize, shared_values: &[i32]) -> Result<ChunkIndex> {
        self.next_chunk_sequence_number += 1; // AddEmptyChunk_57 bumps +0x188 itself
        let seq = self.next_chunk_sequence_number;
        let chunk = Chunk::new(arch as i32, seq);
        let slot = match self.free_chunk_slots.pop() {
            Some(s) => {
                self.chunks[s as usize] = Some(chunk);
                s
            }
            None => {
                self.chunks.push(Some(chunk));
                (self.chunks.len() - 1) as i32
            }
        };
        let ci = ChunkIndex::new(slot);
        let gsv = self.global_system_version;
        let sa = &mut self.archetypes[arch];
        // AddToChunkList_28: grow by doubling (1 when empty), then ArchetypeChunkData.Add.
        if sa.chunk_data.count() == sa.chunk_data.capacity() {
            let new_cap = if sa.chunk_data.capacity() == 0 {
                1
            } else {
                sa.chunk_data.capacity() * 2
            };
            sa.chunk_data.grow(new_cap)?;
        }
        let pos = sa.chunk_data.add(ci, 0, shared_values, gsv)?;
        // zero the enabled masks and disabled counts for the re-used metadata row
        let n = sa.chunk_data.component_count();
        sa.chunk_data
            .set_enabled_bits_and_hierarchical_data(pos, &vec![[0; 2]; n], &vec![0; n])?;
        // EmptySlotTrackingAddChunk (non-shared branch)
        let empty_pos = sa.chunks_with_empty_slots.len() as i32;
        sa.chunks_with_empty_slots.push(slot);
        let c = self.chunk_mut(ci)?;
        c.list_index = pos as i32;
        c.list_with_empty_slots_index = empty_pos;
        c.count = 0;
        c.flags = 0;
        Ok(ci)
    }

    fn empty_slot_remove(&mut self, arch: usize, ci: ChunkIndex) -> Result<()> {
        let at = self.chunk_mut(ci)?.list_with_empty_slots_index;
        let list = &mut self.archetypes[arch].chunks_with_empty_slots;
        ensure!(
            at >= 0 && (at as usize) < list.len(),
            "chunk not in empty-slot list"
        );
        ensure!(
            list[at as usize] == ci.value,
            "empty-slot list entry mismatch"
        );
        list.swap_remove(at as usize);
        let moved = list.get(at as usize).copied();
        if let Some(m) = moved {
            self.chunk_mut(ChunkIndex::new(m))?
                .list_with_empty_slots_index = at;
        }
        Ok(())
    }

    fn empty_slot_add(&mut self, arch: usize, ci: ChunkIndex) -> Result<()> {
        let list = &mut self.archetypes[arch].chunks_with_empty_slots;
        let at = list.len() as i32;
        list.push(ci.value);
        self.chunk_mut(ci)?.list_with_empty_slots_index = at;
        Ok(())
    }

    /// `GetChunkWithEmptySlots` @ 0x8fb9d80 (non-shared): first listed chunk, else a new one.
    fn get_chunk_with_empty_slots(
        &mut self,
        arch: usize,
        shared_values: &[i32],
    ) -> Result<ChunkIndex> {
        for &slot in &self.archetypes[arch].chunks_with_empty_slots {
            let chunk = ChunkIndex::new(slot);
            let list_index = self
                .chunk(chunk)
                .context("missing chunk with free space")?
                .list_index as usize;
            let data = &self.archetypes[arch].chunk_data;
            let mut matches = true;
            for (component, &value) in shared_values.iter().enumerate() {
                matches &= data.get_shared_component_value(component, list_index)? == value;
            }
            if matches {
                return Ok(chunk);
            }
        }
        self.add_empty_chunk(arch, shared_values)
    }

    /// `ChunkDataUtility.SetChunkCount` @ 0x902bc70 (meta-chunk handling omitted).
    fn set_chunk_count(&mut self, arch: usize, ci: ChunkIndex, new_count: i32) -> Result<()> {
        let cap = self.archetypes[arch].archetype.chunk_capacity();
        let (old, list_index) = {
            let c = self.chunk_mut(ci)?;
            (c.count, c.list_index)
        };
        ensure!(new_count != old, "SetChunkCount to the same count ({old})");
        ensure!((0..=cap).contains(&new_count), "chunk count out of range");
        if new_count == 0 {
            return self.release_chunk(arch, ci);
        }
        if new_count == cap {
            self.empty_slot_remove(arch, ci)?;
        } else if old == cap {
            self.empty_slot_add(arch, ci)?;
        }
        self.chunk_mut(ci)?.count = new_count;
        self.archetypes[arch]
            .chunk_data
            .set_chunk_entity_count(list_index as usize, new_count)?;
        Ok(())
    }

    /// `ReleaseChunk` @ 0x902b8a0: leave the empty list (if the chunk was not full), swap-back out of
    /// the archetype chunk list, free the slot.
    fn release_chunk(&mut self, arch: usize, ci: ChunkIndex) -> Result<()> {
        let cap = self.archetypes[arch].archetype.chunk_capacity();
        let (count, list_index) = {
            let c = self.chunk_mut(ci)?;
            (c.count, c.list_index)
        };
        if count < cap {
            self.empty_slot_remove(arch, ci)?;
        }
        let removal = self.archetypes[arch]
            .chunk_data
            .remove_at_swap_back(list_index as usize)?;
        if let Some(moved) = removal.moved {
            self.chunk_mut(moved)?.list_index = list_index;
        }
        self.chunks[ci.value as usize] = None;
        self.free_chunk_slots.push(ci.value);
        Ok(())
    }

    fn bump_order_versions(&mut self, arch: usize) {
        for t in &self.archetypes[arch].archetype.types {
            let i = (t.type_index.value as u32 & TypeIndex::K_INDEX_MASK) as usize;
            self.component_type_order_version[i] += 1;
        }
    }

    // ---- create -----------------------------------------------------------------------------

    /// `CreateEntities` @ 0x8fbb... (`_201`): fill chunks with empty slots until `count` entities
    /// exist. Returns the new handles in creation order.
    pub fn create_entities(&mut self, arch: usize, count: usize) -> Result<Vec<Entity>> {
        let shared_count = self
            .archetypes
            .get(arch)
            .context("unknown archetype")?
            .chunk_data
            .shared_component_count();
        self.create_entities_inner(arch, count, None, &vec![0; shared_count])
    }

    /// Creates entities grouped into chunks by their shared component indices.
    pub fn create_entities_with_shared(
        &mut self,
        arch: usize,
        count: usize,
        shared_values: &[i32],
    ) -> Result<Vec<Entity>> {
        self.create_entities_inner(arch, count, None, shared_values)
    }

    fn create_entities_inner(
        &mut self,
        arch: usize,
        count: usize,
        ids: Option<&[Entity]>,
        shared_values: &[i32],
    ) -> Result<Vec<Entity>> {
        ensure!(arch < self.archetypes.len(), "unknown archetype {arch}");
        ensure!(
            shared_values.len() == self.archetypes[arch].chunk_data.shared_component_count(),
            "shared component count mismatch"
        );
        ensure!(
            shared_values.iter().all(|&value| value >= 0),
            "negative shared component index"
        );
        let mut out = Vec::with_capacity(count);
        let mut remaining = count;
        while remaining > 0 {
            let ci = self.get_chunk_with_empty_slots(arch, shared_values)?;
            let cap = self.archetypes[arch].archetype.chunk_capacity();
            let free = cap - self.chunk_mut(ci)?.count;
            let n = remaining.min(free as usize);
            let done = out.len();
            self.allocate(
                arch,
                ci,
                n as i32,
                &mut out,
                ids.map(|s| &s[done..done + n]),
            )?;
            remaining -= n;
        }
        Ok(out)
    }

    /// Create entities that keep the ids (index and version) a scene stored. Not an original
    /// function: the original deserialiser patches ids via `EntityRemapUtility`; this port-side
    /// helper reproduces the end state (slot `index` live at `version`, every other slot free).
    ///
    /// Every id must have `index >= 0`, `version >= 1`, be unique, and name a currently free slot.
    /// Capacity is grown so the last slot stays free (the invariant `IncreaseCapacity` relies on),
    /// then the free list is rebuilt ascending over all free slots.
    pub fn restore_entities(&mut self, arch: usize, ids: &[Entity]) -> Result<()> {
        let shared_count = self
            .archetypes
            .get(arch)
            .context("unknown archetype")?
            .chunk_data
            .shared_component_count();
        self.restore_entities_with_shared(arch, ids, &vec![0; shared_count])
    }

    /// Restore scene entity IDs while retaining their shared-component group indices.
    pub fn restore_entities_with_shared(
        &mut self,
        arch: usize,
        ids: &[Entity],
        shared_values: &[i32],
    ) -> Result<()> {
        ensure!(arch < self.archetypes.len(), "unknown archetype {arch}");
        ensure!(
            shared_values.len() == self.archetypes[arch].chunk_data.shared_component_count(),
            "shared component count mismatch"
        );
        ensure!(
            shared_values.iter().all(|&value| value >= 0),
            "negative shared component index"
        );
        let mut max = -1i32;
        let mut seen = std::collections::HashSet::new();
        for e in ids {
            ensure!(e.index >= 0 && e.version >= 1, "invalid restored id {e:?}");
            ensure!(seen.insert(e.index), "duplicate entity index {}", e.index);
            max = max.max(e.index);
        }
        if ids.is_empty() {
            return Ok(());
        }
        let needed = max as i64 + 2;
        ensure!(
            needed <= K_MAX_ENTITIES_PER_WORLD,
            "entities per world too high"
        );
        if needed > self.capacity as i64 {
            let mut cap = self.capacity as i64;
            while cap < needed {
                cap *= 2;
            }
            let cap = cap.min(K_MAX_ENTITIES_PER_WORLD) as i32;
            self.resize_and_initialize(self.capacity, cap)?;
        }
        for e in ids {
            ensure!(
                self.entity_in_chunk[e.index as usize].chunk.is_null(),
                "entity slot {} is already live",
                e.index
            );
        }
        ensure!(
            self.entity_in_chunk[(self.capacity - 1) as usize]
                .chunk
                .is_null(),
            "last entity slot is live"
        );
        self.create_entities_inner(arch, ids.len(), Some(ids), shared_values)?;
        // rebuild the free list over all free slots, ascending, last slot terminating with -1
        let free: Vec<i32> = (0..self.capacity)
            .filter(|&i| self.entity_in_chunk[i as usize].chunk.is_null())
            .collect();
        for w in free.windows(2) {
            self.entity_in_chunk[w[0] as usize].index_in_chunk = w[1];
        }
        let last = *free.last().expect("last slot is free");
        self.entity_in_chunk[last as usize].index_in_chunk = -1;
        self.next_free_entity_index = free[0];
        Ok(())
    }

    pub fn create_entity(&mut self, arch: usize) -> Result<Entity> {
        Ok(self.create_entities(arch, 1)?[0])
    }

    /// `ChunkDataUtility.Allocate_42` @ 0x902be20.
    fn allocate(
        &mut self,
        arch: usize,
        ci: ChunkIndex,
        want: i32,
        out: &mut Vec<Entity>,
        ids: Option<&[Entity]>,
    ) -> Result<()> {
        let gsv = self.global_system_version;
        let cap = self.archetypes[arch].archetype.chunk_capacity();
        // AllocateIntoChunk_40: clip to capacity, SetChunkCount, EntityCount += n
        let start = self.chunk_mut(ci)?.count;
        let n = want.min(cap - start);
        if n <= 0 {
            return Ok(());
        }
        self.set_chunk_count(arch, ci, start + n)?;
        self.archetypes[arch].archetype.entity_count += n;
        // AllocateEntities_174
        for i in 0..n {
            let (e, next) = match ids {
                // Restoration path: the id is dictated by the scene; the free list is rebuilt by
                // `restore_entities` afterwards.
                Some(ids) => {
                    let e = ids[i as usize];
                    self.versions[e.index as usize] = e.version;
                    (e, self.next_free_entity_index)
                }
                None => {
                    let mut next =
                        self.entity_in_chunk[self.next_free_entity_index as usize].index_in_chunk;
                    if next == -1 {
                        self.increase_capacity()?;
                        next = self.entity_in_chunk[self.next_free_entity_index as usize]
                            .index_in_chunk;
                    }
                    let idx = self.next_free_entity_index;
                    (Entity::new(idx, self.versions[idx as usize]), next)
                }
            };
            let idx = e.index;
            let off = self.archetypes[arch]
                .archetype
                .layout()
                .entity_offset(0, start + i) as usize;
            let chunk = self.chunk_mut(ci)?;
            chunk.buffer[off..off + 4].copy_from_slice(&e.index.to_le_bytes());
            chunk.buffer[off + 4..off + 8].copy_from_slice(&e.version.to_le_bytes());
            self.entity_in_chunk[idx as usize] = EntityInChunk {
                chunk: ci,
                index_in_chunk: start + i,
            };
            self.entity_order_version += 1;
            self.next_free_entity_index = next;
            out.push(e);
        }
        // InitializeComponents: port zeroes the new slots (defaults of the original not ported).
        let layout = self.archetypes[arch].archetype.layout().clone();
        let chunk = self.chunk_mut(ci)?;
        for t in 1..layout.non_zero_sized_types_count() as usize {
            let sz = layout.size_of(t) as usize;
            let o = layout.offset_of(t) as usize + start as usize * sz;
            chunk.buffer[o..o + n as usize * sz].fill(0);
        }
        let li = self.chunk_mut(ci)?.list_index as usize;
        let stored = &mut self.archetypes[arch];
        for &slot in &stored.archetype.enableable_type_index_in_archetype {
            let slot = slot as usize;
            let mask = unpack_mask(
                stored
                    .chunk_data
                    .get_component_enabled_mask_array_for_type_in_chunk(slot, li)?,
            );
            stored.chunk_data.set_enabled_mask(
                slot,
                li,
                pack_mask(mask | mask_range(start as usize, (start + n) as usize)),
                (start + n) as usize,
            )?;
        }
        self.archetypes[arch]
            .chunk_data
            .set_all_change_version(li, gsv)?;
        self.bump_order_versions(arch);
        Ok(())
    }

    // ---- destroy ----------------------------------------------------------------------------

    /// `DestroyEntities` @ `_203`: stale handles are skipped; contiguous same-chunk runs are
    /// destroyed as one batch. Out-of-range indices are an error (`ValidateEntity`).
    pub fn destroy_entities(&mut self, entities: &[Entity]) -> Result<()> {
        let mut i = 0;
        while i < entities.len() {
            self.validate(entities[i])?;
            let live = |s: &Self, e: Entity| -> Result<Option<EntityInChunk>> {
                s.validate(e)?;
                Ok(if s.versions[e.index as usize] == e.version {
                    Some(s.entity_in_chunk[e.index as usize])
                } else {
                    None
                })
            };
            let first = live(self, entities[i])?;
            let mut run = 1usize;
            match first {
                Some(f) if !f.chunk.is_null() => {
                    while i + run < entities.len() {
                        match live(self, entities[i + run])? {
                            Some(n)
                                if n.chunk == f.chunk
                                    && n.index_in_chunk == f.index_in_chunk + run as i32 =>
                            {
                                run += 1
                            }
                            _ => break,
                        }
                    }
                    self.destroy_batch(f.chunk, f.index_in_chunk, run as i32)?;
                }
                _ => {
                    while i + run < entities.len() && live(self, entities[i + run])?.is_none() {
                        run += 1;
                    }
                }
            }
            i += run;
        }
        Ok(())
    }

    pub fn destroy_entity(&mut self, e: Entity) -> Result<()> {
        self.destroy_entities(&[e])
    }

    /// `ChunkDataUtility.Deallocate_49` @ 0x902d890 with `DeallocateDataEntitiesInChunk_175`.
    fn destroy_batch(&mut self, ci: ChunkIndex, start: i32, count: i32) -> Result<()> {
        let arch = self.chunk_mut(ci)?.archetype_index as usize;
        let layout = self.archetypes[arch].archetype.layout().clone();
        let gsv = self.global_system_version;
        let chunk_count = self.chunk_mut(ci)?.count;
        ensure!(
            start >= 0 && count > 0 && start + count <= chunk_count,
            "batch outside chunk"
        );
        // entities of the batch, in order
        let ent_at = |s: &mut Self, slot: i32| -> Result<i32> {
            let off = layout.entity_offset(0, slot) as usize;
            let b = &s.chunk_mut(ci)?.buffer;
            Ok(i32::from_le_bytes(b[off..off + 4].try_into().unwrap()))
        };
        let mut batch = Vec::with_capacity(count as usize);
        for k in 0..count {
            batch.push(ent_at(self, start + k)?);
        }
        // free backwards: head ends as the batch's first entity
        let mut head = self.next_free_entity_index;
        for &idx in batch.iter().rev() {
            self.external_buffers.remove(&idx);
            let i = idx as usize;
            self.entity_in_chunk[i].chunk = ChunkIndex::NULL;
            self.versions[i] += 1;
            self.entity_in_chunk[i].index_in_chunk = head;
            head = idx;
        }
        self.entity_order_version += 1;
        self.next_free_entity_index = head;
        // swap-back: move the last `n` live entities into the hole
        let n = count.min(chunk_count - start - count);
        if n > 0 {
            let src0 = chunk_count - n;
            for k in 0..n {
                let e = ent_at(self, src0 + k)?;
                self.entity_in_chunk[e as usize].index_in_chunk = start + k;
            }
            let chunk = self.chunk_mut(ci)?;
            for t in 0..layout.non_zero_sized_types_count() as usize {
                let sz = layout.size_of(t) as usize;
                let base = layout.offset_of(t) as usize;
                let from = base + src0 as usize * sz;
                chunk
                    .buffer
                    .copy_within(from..from + n as usize * sz, base + start as usize * sz);
            }
        }
        // Deallocate_49 tail: versions, EntityCount, SetChunkCount
        let li = self.chunk_mut(ci)?.list_index as usize;
        let stored = &mut self.archetypes[arch];
        let new_count = (chunk_count - count) as usize;
        for &slot in &stored.archetype.enableable_type_index_in_archetype {
            let slot = slot as usize;
            let mut mask = unpack_mask(
                stored
                    .chunk_data
                    .get_component_enabled_mask_array_for_type_in_chunk(slot, li)?,
            );
            if n > 0 {
                let src = (chunk_count - n) as usize;
                let dst = start as usize;
                let moved = (mask & mask_range(src, src + n as usize)) >> src;
                mask = (mask & !mask_range(dst, dst + n as usize)) | (moved << dst);
            }
            mask &= if new_count == 0 {
                0
            } else {
                mask_range(0, new_count)
            };
            stored
                .chunk_data
                .set_enabled_mask(slot, li, pack_mask(mask), new_count)?;
        }
        self.bump_order_versions(arch);
        if self.chunk_mut(ci)?.count - count != 0 {
            self.archetypes[arch]
                .chunk_data
                .set_all_change_version(li, gsv)?;
        }
        self.archetypes[arch].archetype.entity_count -= count;
        self.set_chunk_count(arch, ci, chunk_count - count)?;
        Ok(())
    }

    // ---- component bytes & queries ----------------------------------------------------------

    fn locate(&self, e: Entity, t: TypeIndex) -> Result<(ChunkIndex, usize, usize, usize)> {
        ensure!(self.exists(e)?, "entity {:?} does not exist", e);
        let eic = self.entity_in_chunk[e.index as usize];
        let arch = self.chunk(eic.chunk).unwrap().archetype_index as usize;
        let a = &self.archetypes[arch].archetype;
        let ti = a
            .index_of_type(t)
            .ok_or_else(|| anyhow::anyhow!("entity lacks that component"))?;
        let sz = a.layout().size_of(ti) as usize;
        let off = a.layout().entity_offset(ti, eic.index_in_chunk) as usize;
        Ok((eic.chunk, off, sz, arch))
    }

    /// The component's bytes for `e` (empty for zero-sized types).
    pub fn component_bytes(&self, e: Entity, t: TypeIndex) -> Result<&[u8]> {
        let (c, off, sz, _) = self.locate(e, t)?;
        Ok(&self.chunk(c).unwrap().buffer[off..off + sz])
    }

    pub fn component_enabled(&self, entity: Entity, ty: TypeIndex) -> Result<bool> {
        ensure!(ty.is_enableable(), "component is not enableable");
        let (chunk, _, _, _) = self.locate(entity, ty)?;
        let (arch, slot, _, _) = self.chunk_type_slot(chunk, ty)?;
        let row = self.entity_in_chunk[entity.index as usize].index_in_chunk;
        let list_index = self.chunk(chunk).unwrap().list_index as usize;
        let mask = self.archetypes[arch]
            .chunk_data
            .get_component_enabled_mask_array_for_type_in_chunk(slot, list_index)?;
        Ok(unpack_mask(mask) & (1 << row) != 0)
    }

    pub fn set_component_enabled(
        &mut self,
        entity: Entity,
        ty: TypeIndex,
        enabled: bool,
    ) -> Result<()> {
        ensure!(ty.is_enableable(), "component is not enableable");
        let (chunk, _, _, _) = self.locate(entity, ty)?;
        let (arch, slot, _, _) = self.chunk_type_slot(chunk, ty)?;
        let row = self.entity_in_chunk[entity.index as usize].index_in_chunk;
        let chunk = self.chunk(chunk).unwrap();
        let (list_index, count) = (chunk.list_index as usize, chunk.count as usize);
        let data = &mut self.archetypes[arch].chunk_data;
        let mask =
            unpack_mask(data.get_component_enabled_mask_array_for_type_in_chunk(slot, list_index)?);
        let bit = 1u128 << row;
        data.set_enabled_mask(
            slot,
            list_index,
            pack_mask(if enabled { mask | bit } else { mask & !bit }),
            count,
        )?;
        data.set_change_version(slot, list_index, self.global_system_version)
    }

    /// Write a component; bumps the chunk change version to the global system version.
    pub fn set_component_bytes(&mut self, e: Entity, t: TypeIndex, bytes: &[u8]) -> Result<()> {
        let (c, off, sz, arch) = self.locate(e, t)?;
        ensure!(
            bytes.len() == sz,
            "component is {sz} bytes, got {}",
            bytes.len()
        );
        self.chunk_mut(c)?.buffer[off..off + sz].copy_from_slice(bytes);
        if t.is_buffer() {
            self.remove_external_buffer(e, t);
        }
        let li = self.chunk(c).unwrap().list_index as usize;
        let ti = self.archetypes[arch].archetype.index_of_type(t).unwrap();
        let gsv = self.global_system_version;
        self.archetypes[arch]
            .chunk_data
            .set_change_version(ti, li, gsv)?;
        Ok(())
    }

    fn remove_external_buffer(&mut self, entity: Entity, ty: TypeIndex) {
        if let Some(buffers) = self.external_buffers.get_mut(&entity.index) {
            buffers.remove(&ty);
            if buffers.is_empty() {
                self.external_buffers.remove(&entity.index);
            }
        }
    }

    fn buffer_shape(types: &TypeManager, ty: TypeIndex) -> Result<(usize, usize)> {
        ensure!(
            ty.is_buffer() && !ty.is_chunk_component(),
            "expected per-entity buffer type"
        );
        let info = types
            .infos()
            .get(ty.index() as usize)
            .ok_or_else(|| anyhow::anyhow!("unknown buffer type"))?;
        let stride = usize::try_from(info.element_size)?;
        ensure!(stride > 0, "invalid buffer element size");
        Ok((stride, usize::try_from(info.buffer_capacity)?))
    }

    /// Replaces the used buffer elements. Overflow is owned by the store and follows the entity
    /// across chunk compaction; serialized pointer bits are never used as host pointers.
    pub fn set_buffer_payload(
        &mut self,
        types: &TypeManager,
        entity: Entity,
        ty: TypeIndex,
        payload: &[u8],
    ) -> Result<()> {
        let (stride, inline) = Self::buffer_shape(types, ty)?;
        ensure!(
            payload.len().is_multiple_of(stride),
            "partial buffer element"
        );
        let length = payload.len() / stride;
        let capacity = if length <= inline {
            inline
        } else {
            length.max(8)
        };
        let (_, _, size, _) = self.locate(entity, ty)?;
        let header_bytes = crate::buffer::ELEMENT_OFFSET;
        ensure!(
            size >= header_bytes + inline * stride,
            "buffer slot too small"
        );
        let mut scratch = [0; 128];
        let mut large_slot;
        let slot = if let Some(slot) = scratch.get_mut(..size) {
            slot
        } else {
            large_slot = vec![0; size];
            &mut large_slot
        };
        slot[8..12].copy_from_slice(&i32::try_from(length)?.to_le_bytes());
        slot[12..16].copy_from_slice(&i32::try_from(capacity)?.to_le_bytes());
        if length <= inline {
            slot[header_bytes..header_bytes + payload.len()].copy_from_slice(payload);
        }
        self.set_component_bytes(entity, ty, slot)?;
        if length > inline {
            self.external_buffers
                .entry(entity.index)
                .or_default()
                .insert(ty, payload.to_vec());
        }
        Ok(())
    }

    pub fn buffer_payload(
        &self,
        types: &TypeManager,
        entity: Entity,
        ty: TypeIndex,
    ) -> Result<&[u8]> {
        let (stride, inline) = Self::buffer_shape(types, ty)?;
        let slot = self.component_bytes(entity, ty)?;
        ensure!(
            slot.len() >= crate::buffer::ELEMENT_OFFSET,
            "truncated buffer slot"
        );
        let length = usize::try_from(i32::from_le_bytes(slot[8..12].try_into()?))?;
        let capacity = usize::try_from(i32::from_le_bytes(slot[12..16].try_into()?))?;
        ensure!(length <= capacity, "buffer length exceeds capacity");
        let used = length
            .checked_mul(stride)
            .ok_or_else(|| anyhow::anyhow!("buffer size overflow"))?;
        if length <= inline {
            return slot[crate::buffer::ELEMENT_OFFSET..]
                .get(..used)
                .ok_or_else(|| anyhow::anyhow!("inline buffer payload outside slot"));
        }
        self.external_buffers
            .get(&entity.index)
            .and_then(|buffers| buffers.get(&ty))
            .filter(|payload| payload.len() == used)
            .map(Vec::as_slice)
            .ok_or_else(|| anyhow::anyhow!("missing external buffer payload"))
    }

    /// Advance `GlobalSystemVersion` (Unity increments it after each system update so later writes
    /// compare newer than that system's recorded version). Port-side scheduling helper; the
    /// schedule calls it, not user code.
    pub fn increment_global_system_version(&mut self) {
        crate::change_version::increment_global_system_version(&mut self.global_system_version);
    }

    fn chunk_type_slot(&self, c: ChunkIndex, t: TypeIndex) -> Result<(usize, usize, usize, usize)> {
        let ch = self
            .chunk(c)
            .ok_or_else(|| anyhow::anyhow!("chunk {} is not allocated", c.value))?;
        let arch = ch.archetype_index as usize;
        let a = &self.archetypes[arch].archetype;
        let ti = a
            .index_of_type(t)
            .ok_or_else(|| anyhow::anyhow!("chunk lacks that component"))?;
        let sz = a.layout().size_of(ti) as usize;
        let off = a.layout().offset_of(ti) as usize;
        Ok((arch, ti, off, sz * ch.count as usize))
    }

    /// Whether the chunk's archetype holds the component.
    pub fn chunk_has_component(&self, c: ChunkIndex, t: TypeIndex) -> bool {
        self.chunk(c).is_some_and(|ch| {
            self.archetypes[ch.archetype_index as usize]
                .archetype
                .index_of_type(t)
                .is_some()
        })
    }

    /// The component's bytes for every entity of the chunk (`count * size`, slot order).
    pub fn chunk_component_bytes(&self, c: ChunkIndex, t: TypeIndex) -> Result<&[u8]> {
        let (_, _, off, len) = self.chunk_type_slot(c, t)?;
        Ok(&self.chunk(c).unwrap().buffer[off..off + len])
    }

    /// Mutable component array of the chunk; stamps the component's change version with the
    /// current `GlobalSystemVersion` (the `GetComponentDataRW` convention), so call it only when
    /// writing.
    pub fn chunk_component_bytes_mut(&mut self, c: ChunkIndex, t: TypeIndex) -> Result<&mut [u8]> {
        let (arch, ti, off, len) = self.chunk_type_slot(c, t)?;
        let li = self.chunk(c).unwrap().list_index as usize;
        let gsv = self.global_system_version;
        self.archetypes[arch]
            .chunk_data
            .set_change_version(ti, li, gsv)?;
        Ok(&mut self.chunk_mut(c)?.buffer[off..off + len])
    }

    /// Change version of one component in one chunk. Compare through
    /// [`crate::change_version::did_change`] to handle the zero sentinel and wrapping.
    pub fn chunk_change_version(&self, c: ChunkIndex, t: TypeIndex) -> Result<u32> {
        let (arch, ti, _, _) = self.chunk_type_slot(c, t)?;
        let li = self.chunk(c).unwrap().list_index as usize;
        self.archetypes[arch].chunk_data.get_change_version(ti, li)
    }

    /// Like [`Self::component_bytes`] but `None` when the entity lacks the component.
    pub fn try_component_bytes(&self, e: Entity, t: TypeIndex) -> Result<Option<&[u8]>> {
        ensure!(self.exists(e)?, "entity {:?} does not exist", e);
        let eic = self.entity_in_chunk[e.index as usize];
        if !self.chunk_has_component(eic.chunk, t) {
            return Ok(None);
        }
        self.component_bytes(e, t).map(Some)
    }

    /// Structural candidates in archetype then chunk-list order. Individual rows can be filtered
    /// out by enableable terms; use query_entities for row-level results.
    pub fn query_chunks(&self, q: &EntityQuery) -> Vec<ChunkIndex> {
        let mut out = Vec::new();
        for sa in &self.archetypes {
            if !q.matches(&sa.archetype) {
                continue;
            }
            for &c in sa.chunk_data.chunk_indices() {
                if self.chunk(c).is_some_and(|c| c.count > 0) {
                    out.push(c);
                }
            }
        }
        out
    }

    /// Entities stored in a chunk, in slot order.
    pub fn chunk_entities(&self, c: ChunkIndex) -> Result<Vec<Entity>> {
        let ch = self
            .chunk(c)
            .ok_or_else(|| anyhow::anyhow!("chunk not allocated"))?;
        let layout = self.archetypes[ch.archetype_index as usize]
            .archetype
            .layout();
        Ok((0..ch.count)
            .map(|s| {
                let o = layout.entity_offset(0, s) as usize;
                Entity::new(
                    i32::from_le_bytes(ch.buffer[o..o + 4].try_into().unwrap()),
                    i32::from_le_bytes(ch.buffer[o + 4..o + 8].try_into().unwrap()),
                )
            })
            .collect())
    }

    pub fn query_entity_count(&self, query: &EntityQuery) -> Result<usize> {
        self.query_chunks(query)
            .into_iter()
            .try_fold(0, |count, chunk| {
                Ok(count
                    + match self.query_enabled_rows(chunk, query)? {
                        Some(mask) => mask.count_ones() as usize,
                        None => self.chunk(chunk).unwrap().count as usize,
                    })
            })
    }

    fn query_enabled_rows(&self, chunk: ChunkIndex, q: &EntityQuery) -> Result<Option<u128>> {
        let chunk = self.chunk(chunk).unwrap();
        let stored = &self.archetypes[chunk.archetype_index as usize];
        if stored.archetype.enableable_types_count == 0 {
            return Ok(None);
        }
        let mut matched = mask_range(0, chunk.count as usize);
        for (terms, expected) in [(&q.all, true), (&q.none, false)] {
            for &ty in terms.iter().filter(|ty| ty.is_enableable()) {
                if let Some(slot) = stored.archetype.index_of_type(ty) {
                    let mask = unpack_mask(
                        stored
                            .chunk_data
                            .get_component_enabled_mask_array_for_type_in_chunk(
                                slot,
                                chunk.list_index as usize,
                            )?,
                    );
                    matched &= if expected { mask } else { !mask };
                }
            }
        }
        Ok(Some(matched))
    }

    /// Every entity matched by the query.
    pub fn query_entities(&self, q: &EntityQuery) -> Result<Vec<Entity>> {
        let mut out = Vec::new();
        for c in self.query_chunks(q) {
            let Some(matched) = self.query_enabled_rows(c, q)? else {
                out.extend(self.chunk_entities(c)?);
                continue;
            };
            if matched != 0 {
                out.extend(
                    self.chunk_entities(c)?
                        .into_iter()
                        .enumerate()
                        .filter_map(|(row, entity)| (matched & (1 << row) != 0).then_some(entity)),
                );
            }
        }
        Ok(out)
    }
}
