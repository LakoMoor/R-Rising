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
//! Also absent: buffer heap deallocation, enable-bit hierarchical data on moves, entity names, query
//! registration (`RegisterInQueries`), `EntityPatchInfo` rewriting, `InitializeComponents` defaults
//! (new component bytes are zeroed). The query matcher is a port-side minimum, see [`EntityQuery`].

use std::collections::HashMap;

use anyhow::{bail, ensure, Result};

use crate::archetype::{create_archetype_data, Archetype, ArchetypeFlags, SpecialTypes};
use crate::chunk::{Chunk, ChunkIndex};
use crate::entity::Entity;
use crate::type_table::{AccessMode, ComponentType, ComponentTypeInArchetype, TypeIndex, TypeManager};

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
/// must be present. The default exclusion of `Disabled`/`Prefab` archetypes follows Unity's
/// documented `EntityQueryOptions` behaviour and has NOT been checked against the binary's
/// `EntityQueryData` bodies.
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
        if !self.all.iter().all(has) || self.none.iter().any(has) {
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
    special: SpecialTypes,
}

impl EntityComponentStore {
    /// `EntityComponentStore.Create` @ 0x8fa9840: capacity arrays, `GlobalSystemVersion = 1`,
    /// free list chained by `InitializeAdditionalCapacity(0)`.
    pub fn new(initial_capacity: i32, special: SpecialTypes) -> Result<Self> {
        ensure!(initial_capacity >= 1, "entity capacity must be at least 1");
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
            special,
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
        let new_cap = (self.capacity as i64 * 2).min(i64::MAX);
        ensure!(
            new_cap <= K_MAX_ENTITIES_PER_WORLD,
            "entities per world too high: {new_cap} > {K_MAX_ENTITIES_PER_WORLD}"
        );
        let start = if self.capacity > 0 { self.capacity - 1 } else { 0 };
        self.resize_and_initialize(start, new_cap as i32)
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
        ensure!(e.index < self.capacity, "entity index {} outside capacity {}", e.index, self.capacity);
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
    pub fn get_or_create_archetype(&mut self, tm: &TypeManager, types: &[TypeIndex]) -> Result<usize> {
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
        let in_arch: Vec<ComponentTypeInArchetype> =
            list.iter().copied().map(ComponentTypeInArchetype::new).collect();
        let idx = self.archetypes.len();
        let archetype = Archetype::new(idx as i32, in_arch, data);
        let l = archetype.layout();
        if l.num_shared_components() > 0
            || l.num_managed_components() > 0
            || l.num_chunk_components(archetype.types_count) > 0
            || archetype.types.iter().any(|t| t.is_cleanup_component())
        {
            bail!("archetypes with shared/managed/chunk/cleanup components are not supported yet");
        }
        ensure!(l.chunk_capacity > 0, "archetype has zero chunk capacity");
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
    fn add_empty_chunk(&mut self, arch: usize) -> Result<ChunkIndex> {
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
            let new_cap = if sa.chunk_data.capacity() == 0 { 1 } else { sa.chunk_data.capacity() * 2 };
            sa.chunk_data.grow(new_cap)?;
        }
        let pos = sa.chunk_data.add(ci, 0, &[], gsv)?;
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
        ensure!(at >= 0 && (at as usize) < list.len(), "chunk not in empty-slot list");
        ensure!(list[at as usize] == ci.value, "empty-slot list entry mismatch");
        list.swap_remove(at as usize);
        let moved = list.get(at as usize).copied();
        if let Some(m) = moved {
            self.chunk_mut(ChunkIndex::new(m))?.list_with_empty_slots_index = at;
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
    fn get_chunk_with_empty_slots(&mut self, arch: usize) -> Result<ChunkIndex> {
        if let Some(&c) = self.archetypes[arch].chunks_with_empty_slots.first() {
            return Ok(ChunkIndex::new(c));
        }
        self.add_empty_chunk(arch)
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
        let removal = self.archetypes[arch].chunk_data.remove_at_swap_back(list_index as usize)?;
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
        ensure!(arch < self.archetypes.len(), "unknown archetype {arch}");
        let mut out = Vec::with_capacity(count);
        let mut remaining = count;
        while remaining > 0 {
            let ci = self.get_chunk_with_empty_slots(arch)?;
            let cap = self.archetypes[arch].archetype.chunk_capacity();
            let free = cap - self.chunk_mut(ci)?.count;
            let n = remaining.min(free as usize);
            self.allocate(arch, ci, n as i32, &mut out)?;
            remaining -= n;
        }
        Ok(out)
    }

    pub fn create_entity(&mut self, arch: usize) -> Result<Entity> {
        Ok(self.create_entities(arch, 1)?[0])
    }

    /// `ChunkDataUtility.Allocate_42` @ 0x902be20.
    fn allocate(&mut self, arch: usize, ci: ChunkIndex, want: i32, out: &mut Vec<Entity>) -> Result<()> {
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
            let mut next = self.entity_in_chunk[self.next_free_entity_index as usize].index_in_chunk;
            if next == -1 {
                self.increase_capacity()?;
                next = self.entity_in_chunk[self.next_free_entity_index as usize].index_in_chunk;
            }
            let idx = self.next_free_entity_index;
            let version = self.versions[idx as usize];
            let e = Entity::new(idx, version);
            let off = self.archetypes[arch].archetype.layout().entity_offset(0, start + i) as usize;
            let chunk = self.chunk_mut(ci)?;
            chunk.buffer[off..off + 4].copy_from_slice(&e.index.to_le_bytes());
            chunk.buffer[off + 4..off + 8].copy_from_slice(&e.version.to_le_bytes());
            self.entity_in_chunk[idx as usize] = EntityInChunk { chunk: ci, index_in_chunk: start + i };
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
        self.archetypes[arch].chunk_data.set_all_change_version(li, gsv)?;
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
                            Some(n) if n.chunk == f.chunk && n.index_in_chunk == f.index_in_chunk + run as i32 => run += 1,
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
        ensure!(start >= 0 && count > 0 && start + count <= chunk_count, "batch outside chunk");
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
                chunk.buffer.copy_within(from..from + n as usize * sz, base + start as usize * sz);
            }
        }
        // Deallocate_49 tail: versions, EntityCount, SetChunkCount
        let li = self.chunk_mut(ci)?.list_index as usize;
        self.bump_order_versions(arch);
        if self.chunk_mut(ci)?.count - count != 0 {
            self.archetypes[arch].chunk_data.set_all_change_version(li, gsv)?;
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
        let ti = a.index_of_type(t).ok_or_else(|| anyhow::anyhow!("entity lacks that component"))?;
        let sz = a.layout().size_of(ti) as usize;
        let off = a.layout().entity_offset(ti, eic.index_in_chunk) as usize;
        Ok((eic.chunk, off, sz, arch))
    }

    /// The component's bytes for `e` (empty for zero-sized types).
    pub fn component_bytes(&self, e: Entity, t: TypeIndex) -> Result<&[u8]> {
        let (c, off, sz, _) = self.locate(e, t)?;
        Ok(&self.chunk(c).unwrap().buffer[off..off + sz])
    }

    /// Write a component; bumps the chunk change version to the global system version.
    pub fn set_component_bytes(&mut self, e: Entity, t: TypeIndex, bytes: &[u8]) -> Result<()> {
        let (c, off, sz, arch) = self.locate(e, t)?;
        ensure!(bytes.len() == sz, "component is {sz} bytes, got {}", bytes.len());
        self.chunk_mut(c)?.buffer[off..off + sz].copy_from_slice(bytes);
        let li = self.chunk(c).unwrap().list_index as usize;
        let ti = self.archetypes[arch].archetype.index_of_type(t).unwrap();
        let gsv = self.global_system_version;
        self.archetypes[arch].chunk_data.set_change_version(ti, li, gsv)?;
        Ok(())
    }

    /// Non-empty chunks of every archetype matching the query, in archetype then chunk-list order.
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
        let ch = self.chunk(c).ok_or_else(|| anyhow::anyhow!("chunk not allocated"))?;
        let layout = self.archetypes[ch.archetype_index as usize].archetype.layout();
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

    /// Every entity matched by the query.
    pub fn query_entities(&self, q: &EntityQuery) -> Result<Vec<Entity>> {
        let mut out = Vec::new();
        for c in self.query_chunks(q) {
            out.extend(self.chunk_entities(c)?);
        }
        Ok(out)
    }
}
