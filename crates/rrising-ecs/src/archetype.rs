//! `Unity.Entities.Archetype` — the per-type-set record that owns an entity count, a chunk list and
//! the byte layout every chunk of that type set shares.
//!
//! Everything here is translated from the shipped binary:
//! - field names and offsets: `dump.cs`, `internal struct Archetype // TypeDefIndex: 24039`
//!   (`TypeSize: 0x160`, plus a 128-byte `QueryMaskArray` fixed buffer at `0x160`).
//! - the layout arithmetic: `Unity_Entities__Unity_Entities__EntityComponentStore__CreateArchetype_119`
//!   @ 0x8fb0b30, cited below as *CreateArchetype*.
//! - the section-boundary and flag getters: `Unity_Entities__Unity_Entities__Archetype__*` @
//!   0x911c350–0x911cc70, cited as *Archetype getters*.
//! - the chunk-list bookkeeping: `AddToChunkList` @ 0x911c660, `RemoveFromChunkList` @ 0x911c7d0,
//!   `AddToChunkListWithEmptySlots` @ 0x911c870, `RemoveFromChunkListWithEmptySlots` @ 0x911c8f0,
//!   `EmptySlotTrackingRemoveChunk` @ 0x911c940, `EmptySlotTrackingAddChunk` @ 0x911c9b0,
//!   `GetExistingChunkWithEmptySlots` @ 0x911ca20.
//!
//! Three deliberate substitutions, all recorded in `docs/PORT.md`:
//! - the original's `Archetype*` links (`NextChangedArchetype`, `CopyArchetype`,
//!   `InstantiateArchetype`, `CleanupResidueArchetype`, `MetaChunkArchetype`) become archetype
//!   indices, with [`NONE`] standing in for null. Rust will not hand out a pointer into a growable
//!   collection of archetypes, and the links are only ever used to look the target up.
//! - the original's `Types`, `Offsets`, `SizeOfs`, `BufferCapacities` and the two memory-order maps
//!   are raw pointers into block allocators; the port keeps `Vec`s with the same contents.
//! - `Archetype.Chunks` is an `ArchetypeChunkData` (an `UnsafeList<ChunkIndex>` plus a parallel
//!   per-chunk back-reference array). The port keeps the chunk indices and stores the back-reference
//!   on the chunk, which is the same information in a different place.

use crate::type_table::{ComponentTypeInArchetype, TypeIndex, TypeInfo};

/// `ArchetypeFlags` — `dump.cs`: `internal enum ArchetypeFlags`, backed by `ushort`.
///
/// The original keeps this as one `ushort` field at `Archetype+0x122` and ORs bits into it while
/// building the archetype (CreateArchetype @ 0x8fb0b30 writes `+0x122` at 0x8fb16?? … 0x8fb1f??).
/// The port keeps the same `ushort` and the same bit values.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArchetypeFlags(pub u16);

impl ArchetypeFlags {
    /// `ArchetypeFlags.CleanupComplete = 1`
    pub const CLEANUP_COMPLETE: Self = Self(1);
    /// `ArchetypeFlags.CleanupNeeded = 2`
    pub const CLEANUP_NEEDED: Self = Self(2);
    /// `ArchetypeFlags.Disabled = 4`
    pub const DISABLED: Self = Self(4);
    /// `ArchetypeFlags.Prefab = 8`
    pub const PREFAB: Self = Self(8);
    /// `ArchetypeFlags.HasChunkHeader = 16`
    pub const HAS_CHUNK_HEADER: Self = Self(16);
    /// `ArchetypeFlags.HasBlobAssetRefs = 32`
    pub const HAS_BLOB_ASSET_REFS: Self = Self(32);
    /// `ArchetypeFlags.HasCompanionComponents = 64`
    pub const HAS_COMPANION_COMPONENTS: Self = Self(64);
    /// `ArchetypeFlags.HasBufferComponents = 128`
    pub const HAS_BUFFER_COMPONENTS: Self = Self(128);
    /// `ArchetypeFlags.HasManagedComponents = 256`
    pub const HAS_MANAGED_COMPONENTS: Self = Self(256);
    /// `ArchetypeFlags.HasManagedEntityRefs = 512`
    pub const HAS_MANAGED_ENTITY_REFS: Self = Self(512);
    /// `ArchetypeFlags.HasWeakAssetRefs = 1024`
    pub const HAS_WEAK_ASSET_REFS: Self = Self(1024);
    /// `ArchetypeFlags.HasSystemInstanceComponents = 2048`
    pub const HAS_SYSTEM_INSTANCE_COMPONENTS: Self = Self(2048);
    /// `ArchetypeFlags.SpawnTag = 4096`
    pub const SPAWN_TAG: Self = Self(4096);
    /// `ArchetypeFlags.DestroyTag = 8192`
    pub const DESTROY_TAG: Self = Self(8192);

    /// No bits set.
    pub const NONE: Self = Self(0);
    /// Every named bit, for round-trip tests.
    pub const ALL_KNOWN: Self = Self(0x3fff);

    /// `Flags & bit != 0`.
    #[inline]
    pub const fn contains(self, bit: Self) -> bool {
        self.0 & bit.0 != 0
    }

    /// `Flags | bit`. CreateArchetype sets each bit with `*(ushort*)(this+0x122) |= n`.
    #[inline]
    pub const fn with(self, bit: Self) -> Self {
        Self(self.0 | bit.0)
    }

    /// `Flags & !bit`.
    #[inline]
    pub const fn without(self, bit: Self) -> Self {
        Self(self.0 & !bit.0)
    }

    /// `Flags | other`.
    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// `Archetype.kQueryMaskBytes` — the size of the `QueryMaskArray` fixed buffer at `Archetype+0x160`
/// (`[FixedBuffer(typeof(byte), 128)]` in `dump.cs`).
pub const K_QUERY_MASK_BYTES: usize = 128;

/// The archetype index that stands in for the original's null `Archetype*`.
pub const NONE: i32 = -1;

/// `Chunk.kChunkBufferSize` / `Chunk.kBufferSize` — the usable bytes of a 32768-byte chunk after the
/// 64-byte header. src: `dump.cs`, `Chunk`.
pub const K_CHUNK_BUFFER_SIZE: i32 = 32704;

/// `Chunk.kMaximumEntitiesPerChunk` — CreateArchetype asserts `0 < ChunkCapacity < 0xff9`
/// (`UnityEngine_Assertions__Assert__IsTrue_2(iVar20 < 0xff9,0)` @ 0x8fb173f).
pub const K_MAXIMUM_ENTITIES_PER_CHUNK: i32 = 4088;

/// `EntityRemapUtility.EntityPatchInfo` — one `(typeIndexInArchetype, offsetWithinEntity)` pair the
/// store walks when an entity moves between chunks and its `Entity` references have to be rewritten.
///
/// src: `dump.cs`, `EntityRemapUtility.EntityPatchInfo` (`TypeSize: 0x8`); CreateArchetype fills the
/// arrays through `EntityRemapUtility__AppendEntityPatches_9` @ 0x8fb1b3e.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityPatchInfo {
    /// `int TypeIndexInArchetype; // 0x0`
    pub type_index_in_archetype: i32,
    /// `int OffsetWithinEntity; // 0x4`
    pub offset_within_entity: i32,
}

/// `EntityRemapUtility.BufferEntityPatchInfo` — the same for a buffer element, which additionally
/// needs the element size and the buffer's capacity to iterate.
///
/// src: `dump.cs`, `EntityRemapUtility.BufferEntityPatchInfo` (`TypeSize: 0x10`); CreateArchetype
/// fills it through `AppendBufferEntityPatches_10` @ 0x8fb1b77.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferEntityPatchInfo {
    /// `int TypeIndexInArchetype; // 0x0`
    pub type_index_in_archetype: i32,
    /// `int OffsetWithinEntity; // 0x4`
    pub offset_within_entity: i32,
    /// `int ElementSize; // 0x8`
    pub element_size: i32,
    /// `int BufferCapacity; // 0xc`
    pub buffer_capacity: i32,
}

/// The six special `TypeIndex` slots CreateArchetype compares against. `EntityComponentStore` holds
/// them at `+0x1ac … +0x1c8` (`dump.cs`); a type equal to one of these sets the matching
/// `ArchetypeFlags` bit.
///
/// There is deliberately no derived all-zero `Default`: `TypeIndex(0)` is a real type (the reserved
/// null / first registered type) and would make every slot alias it, setting Disabled/Prefab/...
/// flags on every archetype. [`SpecialTypes::new`] and `Default` leave every slot at
/// [`SpecialTypes::UNSET`] (`TypeIndex(-1)`, matching no registered type).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecialTypes {
    /// `TypeIndex m_ChunkHeaderType; // 0x1ac` → `HasChunkHeader`
    pub chunk_header: TypeIndex,
    /// `TypeIndex m_PrefabType; // 0x1b0` → `Prefab`
    pub prefab: TypeIndex,
    /// `TypeIndex m_SpawnTagType; // 0x1b4` → `SpawnTag`
    pub spawn_tag: TypeIndex,
    /// `TypeIndex m_DestroyTagType; // 0x1b8` → `DestroyTag`
    pub destroy_tag: TypeIndex,
    /// `TypeIndex m_CleanupEntityType; // 0x1bc` → `CleanupComplete`
    pub cleanup_entity: TypeIndex,
    /// `TypeIndex m_DisabledType; // 0x1c0` → `Disabled`
    pub disabled: TypeIndex,
    /// `TypeIndex m_EntityType; // 0x1c4`
    pub entity_type: TypeIndex,
    /// `TypeIndex m_SystemInstanceType; // 0x1c8` → `HasSystemInstanceComponents`
    pub system_instance: TypeIndex,
}

impl SpecialTypes {
    /// A slot that matches no registered type (negative values are never valid `TypeIndex`es).
    pub const UNSET: TypeIndex = TypeIndex { value: -1 };

    /// Only the Entity type is known; every other special slot is [`Self::UNSET`].
    pub const fn new(entity_type: TypeIndex) -> Self {
        Self {
            chunk_header: Self::UNSET,
            prefab: Self::UNSET,
            spawn_tag: Self::UNSET,
            destroy_tag: Self::UNSET,
            cleanup_entity: Self::UNSET,
            disabled: Self::UNSET,
            entity_type,
            system_instance: Self::UNSET,
        }
    }

    /// True when the Entity slot was never set (the store refuses such a configuration).
    pub const fn entity_type_is_set(&self) -> bool {
        self.entity_type.value >= 0
    }
}

impl Default for SpecialTypes {
    /// All slots [`Self::UNSET`], including `entity_type`, which must be set before use.
    fn default() -> Self {
        Self::new(Self::UNSET)
    }
}

/// `align64` — CreateArchetype calls `CollectionHelper.CheckIntPositivePowerOfTwo(0x40)` and then
/// `(x + 0x3f) & ~0x3f` for every alignment inside a chunk (0x8fb15e7, 0x8fb17d5, 0x8fb1a49).
const fn align64(x: i32) -> i32 {
    (x + 0x3f) & !0x3f
}

/// The FNV-1a 64-bit prime `0x100000001b3`, the multiplier CreateArchetype uses for the archetype
/// hash at 0x8fb2077: `uVar26 = (uVar26 ^ TypeInfo[i].StableTypeHash) * 0x100000001b3`.
const FNV_PRIME_64: u64 = 0x100000001b3;

/// CreateArchetype @ 0x8fb18c8 stores `TypeIndex.Value & 0xffffc000`
/// as the first memory-order sort key. Every flag participates; reducing it to
/// component categories loses the reference/enableable/other ordering bits.
fn memory_order_flags(value: i32) -> i32 {
    (value as u32 & TypeIndex::K_FLAGS_MASK) as i32
}

/// The per-archetype layout numbers CreateArchetype computes: where each type's block sits inside a
/// chunk, how many entities fit, and how big one entity is.
///
/// A chunk is a flat 32704-byte buffer and each type gets its own block in it:
///
/// ```text
/// Offsets[i] = running;  running += align64(SizeOfs[i] * ChunkCapacity)
/// ```
///
/// so type `i`'s block spans `Offsets[i] .. Offsets[i] + align64(SizeOfs[i] * ChunkCapacity)` and
/// entity `e` of that type is at `Offsets[i] + e * SizeOfs[i]`. The walk that fills `Offsets` runs in
/// *memory order* (`TypeMemoryOrderIndexToIndexInArchetype[i]`) while the array is stored indexed by
/// *index in archetype* — the physical order inside a chunk is the memory order, not the archetype's
/// type order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchetypeLayout {
    /// `short FirstBufferComponent; // 0x118` — index of the first buffer type.
    pub first_buffer_component: i32,
    /// `short FirstManagedComponent; // 0x11a`
    pub first_managed_component: i32,
    /// `short FirstTagComponent; // 0x11c` — also `NonZeroSizedTypesCount`.
    pub first_tag_component: i32,
    /// `short FirstSharedComponent; // 0x11e`
    pub first_shared_component: i32,
    /// `short FirstChunkComponent; // 0x120`
    pub first_chunk_component: i32,
    /// `int ChunkCapacity; // 0xc0` — entities per chunk.
    pub chunk_capacity: i32,
    /// `int InstanceSize; // 0xcc` — `sum(SizeOfs[0 .. NonZeroSizedTypesCount])`, unaligned.
    pub instance_size: i32,
    /// `int InstanceSizeWithOverhead; // 0xd0` — the same sum with every term aligned to 64.
    pub instance_size_with_overhead: i32,
    /// `int* Offsets; // 0x100` — one byte offset per type, indexed by index in archetype.
    pub offsets: Vec<i32>,
    /// `ushort* SizeOfs; // 0x108` — bytes one entity of that type occupies. Zero at and past
    /// `NonZeroSizedTypesCount`, which is why the tag/shared/chunk section costs nothing per entity.
    pub sizeofs: Vec<u16>,
    /// `int* BufferCapacities; // 0x110` — `TypeInfo.BufferCapacity`, zero for non-buffers.
    pub buffer_capacities: Vec<i32>,
    /// `int* TypeMemoryOrderIndexToIndexInArchetype; // 0xf0`
    pub memory_order_to_archetype: Vec<i32>,
    /// `int* TypeIndexInArchetypeToMemoryOrderIndex; // 0xf8`
    pub archetype_to_memory_order: Vec<i32>,
}

impl ArchetypeLayout {
    /// `Archetype.NativeComponentsEnd` — src: `get_NativeComponentsEnd_19` @ 0x911c4a0 returns
    /// `FirstBufferComponent`.
    #[inline]
    pub const fn native_components_end(&self) -> i32 {
        self.first_buffer_component
    }

    /// `Archetype.NumNativeComponentData` — src: `get_NumNativeComponentData_12` @ 0x911c3f0:
    /// `*(short*)(this + 0x118) + -1`.
    ///
    /// The shipped getter really does subtract one where every sibling getter
    /// (`NumBufferComponents` @ 0x911c400, `NumManagedComponents` @ 0x911c420,
    /// `NumTagComponents` @ 0x911c440, `NumSharedComponents` @ 0x911c460) is a plain difference of
    /// two boundaries. The port keeps the binary's arithmetic rather than "fixing" it, and the
    /// discrepancy is recorded in `docs/PORT.md`.
    #[inline]
    pub const fn num_native_component_data(&self) -> i32 {
        self.first_buffer_component - 1
    }

    /// `Archetype.NumBufferComponents` — src: `get_NumBufferComponents_13` @ 0x911c400:
    /// `FirstManagedComponent - FirstBufferComponent`.
    #[inline]
    pub const fn num_buffer_components(&self) -> i32 {
        self.first_managed_component - self.first_buffer_component
    }

    /// `Archetype.NumManagedComponents` — src: `get_NumManagedComponents_14` @ 0x911c420:
    /// `FirstTagComponent - FirstManagedComponent`.
    #[inline]
    pub const fn num_managed_components(&self) -> i32 {
        self.first_tag_component - self.first_managed_component
    }

    /// `Archetype.NumTagComponents` — src: `get_NumTagComponents_15` @ 0x911c440:
    /// `FirstSharedComponent - FirstTagComponent`.
    #[inline]
    pub const fn num_tag_components(&self) -> i32 {
        self.first_shared_component - self.first_tag_component
    }

    /// `Archetype.NumSharedComponents` — src: `get_NumSharedComponents_16` @ 0x911c460:
    /// `FirstChunkComponent - FirstSharedComponent`.
    #[inline]
    pub const fn num_shared_components(&self) -> i32 {
        self.first_chunk_component - self.first_shared_component
    }

    /// `Archetype.NumChunkComponents` — src: `get_NumChunkComponents_17` @ 0x911c480:
    /// `TypesCount - FirstChunkComponent`. `TypesCount` is passed in because it lives on the
    /// archetype, not on the layout.
    #[inline]
    pub const fn num_chunk_components(&self, types_count: i32) -> i32 {
        types_count - self.first_chunk_component
    }

    /// `Archetype.NonZeroSizedTypesCount` — src: `get_NonZeroSizedTypesCount_18` @ 0x911c490:
    /// `FirstTagComponent`. Everything at or past it costs zero bytes per entity.
    #[inline]
    pub const fn non_zero_sized_types_count(&self) -> i32 {
        self.first_tag_component
    }

    /// `Archetype.BufferComponentsEnd` — src: `get_BufferComponentsEnd_20` @ 0x911c4b0.
    #[inline]
    pub const fn buffer_components_end(&self) -> i32 {
        self.first_managed_component
    }

    /// `Archetype.TagComponentsEnd` — src: `get_TagComponentsEnd_22` @ 0x911c4c0.
    #[inline]
    pub const fn tag_components_end(&self) -> i32 {
        self.first_shared_component
    }

    /// `Archetype.SharedComponentsEnd` — src: `get_SharedComponentsEnd_23` @ 0x911c4d0.
    #[inline]
    pub const fn shared_components_end(&self) -> i32 {
        self.first_chunk_component
    }

    /// Bytes one entity of type `i` occupies, `SizeOfs[i]`.
    #[inline]
    pub fn size_of(&self, type_index_in_archetype: usize) -> u16 {
        self.sizeofs[type_index_in_archetype]
    }

    /// Byte offset of type `i`'s block inside a chunk, `Offsets[i]`.
    #[inline]
    pub fn offset_of(&self, type_index_in_archetype: usize) -> i32 {
        self.offsets[type_index_in_archetype]
    }

    /// Where entity `slot` of type `i` lives inside its chunk.
    ///
    /// The original spells this inline as `*(int*)(Offsets + type*4) + slot * *(ushort*)(SizeOfs +
    /// type*2)`; `ChunkDataUtility` and the `ArchetypeChunk` accessors all compute it that way.
    #[inline]
    pub fn entity_offset(&self, type_index_in_archetype: usize, slot: i32) -> i32 {
        self.offsets[type_index_in_archetype] + slot * self.sizeofs[type_index_in_archetype] as i32
    }
}

/// Everything CreateArchetype derives from a type list, before the archetype gets an index, a chunk
/// list and query registrations.
///
/// src: CreateArchetype @ 0x8fb0b30, in the order the body performs it:
/// 1. sort the type indices into memory order (insertion sort on `(typeFlags, memoryOrderings)`);
/// 2. walk the end of the list backwards to find the category boundaries;
/// 3. copy `SizeInChunk` / `BufferCapacity` per type and take the minimum `MaximumChunkCapacity`;
/// 4. `ChunkCapacity = 32704 / sum(SizeOfs)`, decremented until
///    `sum(align64(SizeOfs[i] * capacity)) <= 32704`, then capped by that minimum;
/// 5. `InstanceSize = sum(SizeOfs[0 .. NonZeroSized])`,
///    `InstanceSizeWithOverhead = sum(align64(SizeOfs[i]))`;
/// 6. `Offsets[indexInArchetype] = running; running += align64(SizeOfs * capacity)` in memory order;
/// 7. OR the flag bits, OR the per-type `BloomFilterMask`, and FNV-combine the `StableTypeHash`es;
/// 8. collect the entity-reference patch lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchetypeData {
    /// `ulong StableHash; // 0xe0`
    pub stable_hash: u64,
    /// `ulong BloomFilterMask; // 0xe8` — the OR of every type's `TypeInfo.BloomFilterMask`.
    pub bloom_filter_mask: u64,
    /// `ArchetypeFlags Flags; // 0x122`
    pub flags: ArchetypeFlags,
    /// `EntityRemapUtility.EntityPatchInfo* ScalarEntityPatches; // 0x148`, length
    /// `int ScalarEntityPatchCount; // 0xd4`.
    pub scalar_entity_patches: Vec<EntityPatchInfo>,
    /// `EntityRemapUtility.BufferEntityPatchInfo* BufferEntityPatches; // 0x150`, length
    /// `int BufferEntityPatchCount; // 0xd8`.
    pub buffer_entity_patches: Vec<BufferEntityPatchInfo>,
    pub layout: ArchetypeLayout,
}

/// Derive one archetype's layout, flags and hashes from its type set.
///
/// `types` is the type list in the order the archetype will store it. The caller has already decided
/// which archetype this set of types means — that lookup is the store's job.
pub fn create_archetype_data(
    types: &[TypeIndex],
    infos: &[TypeInfo],
    special: &SpecialTypes,
) -> anyhow::Result<ArchetypeData> {
    let count = types.len();
    anyhow::ensure!(count <= i32::MAX as usize, "archetype has too many types");

    // Validate the indices once up front so every lookup below can be a plain index.
    for &t in types {
        anyhow::ensure!(
            (t.index() as usize) < infos.len(),
            "type index {} is not registered",
            t.index()
        );
    }

    // 1. memory order.
    let mut memory_order: Vec<i32> = (0..count as i32).collect();
    memory_order.sort_by(|&a, &b| {
        let ka = memory_order_flags(types[a as usize].value);
        let kb = memory_order_flags(types[b as usize].value);
        ka.cmp(&kb).then_with(|| {
            let ia = infos[types[a as usize].index() as usize].memory_ordering;
            let ib = infos[types[b as usize].index() as usize].memory_ordering;
            ia.cmp(&ib)
        })
    });

    let mut archetype_to_memory_order = vec![0i32; count];
    for (memory_index, &archetype_index) in memory_order.iter().enumerate() {
        archetype_to_memory_order[archetype_index as usize] = memory_index as i32;
    }

    // 2. category boundaries, walking the end of the list backwards as the body does.
    let mut first_chunk = count as i32;
    while first_chunk > 0 && types[first_chunk as usize - 1].is_chunk_component() {
        first_chunk -= 1;
    }
    let mut first_shared = first_chunk;
    while first_shared > 0 && types[first_shared as usize - 1].is_shared_component_type() {
        first_shared -= 1;
    }
    let mut first_tag = first_shared;
    while first_tag > 0 && types[first_tag as usize - 1].is_zero_sized() {
        first_tag -= 1;
    }
    let mut first_managed = first_tag;
    while first_managed > 0 && types[first_managed as usize - 1].is_managed_component() {
        first_managed -= 1;
    }
    let mut first_buffer = first_managed;
    while first_buffer > 0 && types[first_buffer as usize - 1].is_buffer() {
        first_buffer -= 1;
    }

    // 3. per-type sizes, plus the smallest `MaximumChunkCapacity` over *all* types — the body reads
    //    it for every type, not only the non-zero-sized ones (0x8fb14fa).
    let mut sizeofs = vec![0u16; count];
    let mut buffer_capacities = vec![0i32; count];
    let mut min_max_capacity = i32::MAX;
    for (i, &t) in types.iter().enumerate() {
        let info = &infos[t.index() as usize];
        if (i as i32) < first_tag {
            // `ThrowIfComponentDataSizeIsLargerThanShortMaxValue_118` @ 0x8fb1480 guards this cast.
            anyhow::ensure!(
                info.size_in_chunk <= u16::MAX as i32,
                "component data size {} exceeds short max for type {}",
                info.size_in_chunk,
                info.name
            );
            sizeofs[i] = info.size_in_chunk as u16;
            buffer_capacities[i] = info.buffer_capacity;
        }
        if min_max_capacity > info.maximum_chunk_capacity {
            min_max_capacity = info.maximum_chunk_capacity;
        }
    }

    // 4. chunk capacity: `0x7fc0 / totalSize` @ 0x8fb154f, then the loop at 0x8fb1585 that walks the
    //    capacity down until the aligned blocks fit, then the `min` with the per-type cap @ 0x8fb1641.
    let total_size: i32 = sizeofs[..first_tag as usize]
        .iter()
        .map(|&s| s as i32)
        .sum();
    let mut capacity = if total_size > 0 {
        K_CHUNK_BUFFER_SIZE / total_size
    } else {
        K_MAXIMUM_ENTITIES_PER_CHUNK
    };
    loop {
        let mut used = 0i32;
        for &s in &sizeofs[..first_tag as usize] {
            used += align64(s as i32 * capacity);
        }
        if used <= K_CHUNK_BUFFER_SIZE || capacity <= 0 {
            break;
        }
        capacity -= 1;
    }
    if min_max_capacity < capacity {
        capacity = min_max_capacity;
    }
    anyhow::ensure!(
        capacity > 0 && capacity <= K_MAXIMUM_ENTITIES_PER_CHUNK,
        "chunk capacity {capacity} out of range for an archetype of {count} types"
    );

    // 5. instance sizes (0x8fb17c9 … 0x8fb17d5).
    let mut instance_size = 0i32;
    let mut instance_size_with_overhead = 0i32;
    for &s in &sizeofs[..first_tag as usize] {
        instance_size += s as i32;
        instance_size_with_overhead += align64(s as i32);
    }

    // 6. offsets, walking memory order and storing by index in archetype (0x8fb1a3d … 0x8fb1a49).
    let mut offsets = vec![0i32; count];
    let mut running = 0i32;
    for &archetype_index in &memory_order {
        let i = archetype_index as usize;
        offsets[i] = running;
        running += align64(sizeofs[i] as i32 * capacity);
    }

    let layout = ArchetypeLayout {
        first_buffer_component: first_buffer,
        first_managed_component: first_managed,
        first_tag_component: first_tag,
        first_shared_component: first_shared,
        first_chunk_component: first_chunk,
        chunk_capacity: capacity,
        instance_size,
        instance_size_with_overhead,
        offsets,
        sizeofs,
        buffer_capacities,
        memory_order_to_archetype: memory_order.clone(),
        archetype_to_memory_order,
    };

    // 7. flags, bloom mask, stable hash. The body derives the per-type flags in one forward pass
    //    (0x8fb1267 … 0x8fb1a??) and the two count-derived bits after it.
    let mut flags = ArchetypeFlags::NONE;
    let mut bloom = 0u64;
    for &t in types {
        let info = &infos[t.index() as usize];
        bloom |= info.bloom_filter_mask;

        if t == special.chunk_header {
            flags = flags.with(ArchetypeFlags::HAS_CHUNK_HEADER);
        }
        if t == special.prefab {
            flags = flags.with(ArchetypeFlags::PREFAB);
        }
        if t == special.spawn_tag {
            flags = flags.with(ArchetypeFlags::SPAWN_TAG);
        }
        if t == special.destroy_tag {
            flags = flags.with(ArchetypeFlags::DESTROY_TAG);
        }
        if t == special.disabled {
            flags = flags.with(ArchetypeFlags::DISABLED);
        }
        if t == special.system_instance {
            flags = flags.with(ArchetypeFlags::HAS_SYSTEM_INSTANCE_COMPONENTS);
        }
        if info.has_blob_asset_refs() {
            flags = flags.with(ArchetypeFlags::HAS_BLOB_ASSET_REFS);
        }
        // `((packed & 0x20000000) == 0) && ((packed & 0x38000000) == 0x10000000) && Category == 4`
        if !t.is_chunk_component() && t.is_managed_component() && info.category as i32 == 4 {
            flags = flags.with(ArchetypeFlags::HAS_COMPANION_COMPONENTS);
        }
        // `(packed & 0x38000000) == 0x10000000 && ((value >> 0x11) & 1) == 0`
        if t.is_managed_component() && t.has_entity_references() {
            flags = flags.with(ArchetypeFlags::HAS_MANAGED_COMPONENTS);
        }
        if info.has_weak_asset_refs() {
            flags = flags.with(ArchetypeFlags::HAS_WEAK_ASSET_REFS);
        }
    }
    // `NonZeroSizedTypesCount - FirstManagedComponent > 0` → HasManagedEntityRefs
    if first_tag - first_managed > 0 {
        flags = flags.with(ArchetypeFlags::HAS_MANAGED_ENTITY_REFS);
    }
    // `FirstManagedComponent - FirstBufferComponent > 0` → HasBufferComponents
    if first_managed - first_buffer > 0 {
        flags = flags.with(ArchetypeFlags::HAS_BUFFER_COMPONENTS);
    }

    // The hash walks memory order, so it does not depend on the order the caller listed the types in.
    let mut hash = 1u64;
    for &archetype_index in &memory_order {
        let info = &infos[types[archetype_index as usize].index() as usize];
        hash = (hash ^ info.stable_type_hash).wrapping_mul(FNV_PRIME_64);
    }

    // 8. entity-reference patches. The body walks the list in archetype order and reads
    //    `TypeInfo.EntityOffsetStartIndex` / `EntityOffsetCount` out of the store's
    //    `m_EntityOffsetInfos` @ 0x1f0. A negative `BufferCapacity` is the "not a buffer" test.
    let mut scalar_patches = Vec::new();
    let mut buffer_patches = Vec::new();
    for (i, &t) in types.iter().enumerate() {
        let info = &infos[t.index() as usize];
        if info.buffer_capacity < 0 {
            // `(*packed & 0x38000000) != 0x10000000 && SizeInChunk > 0` @ 0x8fb1b3e
            if !t.is_managed_component() && info.size_in_chunk > 0 {
                for k in 0..info.entity_offset_count {
                    scalar_patches.push(EntityPatchInfo {
                        type_index_in_archetype: i as i32,
                        offset_within_entity: k,
                    });
                }
            }
        } else {
            for k in 0..info.entity_offset_count {
                buffer_patches.push(BufferEntityPatchInfo {
                    type_index_in_archetype: i as i32,
                    offset_within_entity: k,
                    element_size: info.element_size,
                    buffer_capacity: info.buffer_capacity,
                });
            }
        }
    }

    Ok(ArchetypeData {
        stable_hash: hash,
        bloom_filter_mask: bloom,
        flags,
        scalar_entity_patches: scalar_patches,
        buffer_entity_patches: buffer_patches,
        layout,
    })
}

/// One entry in the store's archetype list: the derived data plus the identity, chunk list and query
/// registration the archetype owns.
///
/// src: `dump.cs`, `internal struct Archetype // TypeDefIndex: 24039`.
#[derive(Clone, Debug)]
pub struct Archetype {
    /// `int ArchetypeIndex; // 0xb8`
    pub archetype_index: i32,
    /// `int EntityCount; // 0xbc` — live entities across all chunks of this archetype.
    pub entity_count: i32,
    /// `int TypesCount; // 0xc4`
    pub types_count: i32,
    /// `int EnableableTypesCount; // 0xc8`
    pub enableable_types_count: i32,
    /// `ComponentTypeInArchetype* Types; // 0x70` — the type list, in archetype order.
    pub types: Vec<ComponentTypeInArchetype>,
    /// `int* EnableableTypeIndexInArchetype; // 0x78` — archetype indices of the enableable types,
    /// in the order they appear in `types`.
    pub enableable_type_index_in_archetype: Vec<i32>,
    /// The derived layout, flags and hashes.
    pub data: ArchetypeData,
    /// `ArchetypeChunkData Chunks; // 0x0` — the chunk list.
    pub chunks: Vec<i32>,
    /// `UnsafeList<ChunkIndex> ChunksWithEmptySlots; // 0x18` — positions in `chunks`.
    pub chunks_with_empty_slots: Vec<i32>,
    /// `Archetype* NextChangedArchetype; // 0xb0` — the changed-archetype free-list link.
    pub next_changed_archetype: i32,
    /// `Archetype* CopyArchetype; // 0x128`
    pub copy_archetype: i32,
    /// `Archetype* InstantiateArchetype; // 0x130`
    pub instantiate_archetype: i32,
    /// `Archetype* CleanupResidueArchetype; // 0x138`
    pub cleanup_residue_archetype: i32,
    /// `Archetype* MetaChunkArchetype; // 0x140`
    pub meta_chunk_archetype: i32,
    /// `bool RegisteredWithQueries; // 0x124`
    pub registered_with_queries: bool,
    /// `byte[128] QueryMaskArray; // 0x160` — one bit per registered query, addressed by byte.
    pub query_mask_array: [u8; K_QUERY_MASK_BYTES],
}

impl Archetype {
    /// Start a new archetype record from derived data.
    pub fn new(
        archetype_index: i32,
        types: Vec<ComponentTypeInArchetype>,
        data: ArchetypeData,
    ) -> Self {
        let types_count = types.len() as i32;
        let enableable_type_index_in_archetype = types
            .iter()
            .enumerate()
            .filter(|(_, t)| t.type_index.is_enableable())
            .map(|(i, _)| i as i32)
            .collect::<Vec<_>>();
        let enableable_types_count = enableable_type_index_in_archetype.len() as i32;
        Self {
            archetype_index,
            entity_count: 0,
            types_count,
            enableable_types_count,
            types,
            enableable_type_index_in_archetype,
            data,
            chunks: Vec::new(),
            chunks_with_empty_slots: Vec::new(),
            next_changed_archetype: NONE,
            copy_archetype: NONE,
            instantiate_archetype: NONE,
            cleanup_residue_archetype: NONE,
            meta_chunk_archetype: NONE,
            registered_with_queries: false,
            query_mask_array: [0; K_QUERY_MASK_BYTES],
        }
    }

    #[inline]
    pub const fn layout(&self) -> &ArchetypeLayout {
        &self.data.layout
    }

    #[inline]
    pub const fn flags(&self) -> ArchetypeFlags {
        self.data.flags
    }

    #[inline]
    pub const fn chunk_capacity(&self) -> i32 {
        self.data.layout.chunk_capacity
    }

    #[inline]
    pub const fn instance_size(&self) -> i32 {
        self.data.layout.instance_size
    }

    /// `Archetype.HasChunkHeader` — src: `get_HasChunkHeader_6` @ 0x911c390:
    /// `*(byte*)(this + 0x122) & 0x10`.
    #[inline]
    pub const fn has_chunk_header(&self) -> bool {
        self.data.flags.contains(ArchetypeFlags::HAS_CHUNK_HEADER)
    }

    /// `Archetype.CleanupComplete` — src: `get_CleanupComplete_0` @ 0x911c350, bit 1.
    #[inline]
    pub const fn cleanup_complete(&self) -> bool {
        self.data.flags.contains(ArchetypeFlags::CLEANUP_COMPLETE)
    }

    /// `Archetype.CleanupNeeded` — src: `get_CleanupNeeded_1` @ 0x911c360, bit 2.
    #[inline]
    pub const fn cleanup_needed(&self) -> bool {
        self.data.flags.contains(ArchetypeFlags::CLEANUP_NEEDED)
    }

    /// `Archetype.Disabled` — src: `get_Disabled_2` @ 0x911c370, bit 4.
    #[inline]
    pub const fn disabled(&self) -> bool {
        self.data.flags.contains(ArchetypeFlags::DISABLED)
    }

    /// `Archetype.Prefab` — src: `get_Prefab_3` @ 0x911c380, bit 8.
    #[inline]
    pub const fn prefab(&self) -> bool {
        self.data.flags.contains(ArchetypeFlags::PREFAB)
    }

    /// `Archetype.SpawnTag` — src: `get_SpawnTag_4`, bit 4096.
    #[inline]
    pub const fn spawn_tag(&self) -> bool {
        self.data.flags.contains(ArchetypeFlags::SPAWN_TAG)
    }

    /// `Archetype.DestroyTag` — src: `get_DestroyTag_5`, bit 8192.
    #[inline]
    pub const fn destroy_tag(&self) -> bool {
        self.data.flags.contains(ArchetypeFlags::DESTROY_TAG)
    }

    /// `Archetype.HasBlobAssetRefs` — src: `get_HasBlobAssetRefs_7`, bit 32.
    #[inline]
    pub const fn has_blob_asset_refs(&self) -> bool {
        self.data
            .flags
            .contains(ArchetypeFlags::HAS_BLOB_ASSET_REFS)
    }

    /// `Archetype.HasManagedEntityRefs` — src: `get_HasManagedEntityRefs_8`, bit 512.
    #[inline]
    pub const fn has_managed_entity_refs(&self) -> bool {
        self.data
            .flags
            .contains(ArchetypeFlags::HAS_MANAGED_ENTITY_REFS)
    }

    /// `Archetype.HasCompanionComponents` — src: `get_HasCompanionComponents_9`, bit 64.
    #[inline]
    pub const fn has_companion_components(&self) -> bool {
        self.data
            .flags
            .contains(ArchetypeFlags::HAS_COMPANION_COMPONENTS)
    }

    /// `Archetype.HasWeakAssetRefs` — src: `get_HasWeakAssetRefs_10`, bit 1024.
    #[inline]
    pub const fn has_weak_asset_refs(&self) -> bool {
        self.data
            .flags
            .contains(ArchetypeFlags::HAS_WEAK_ASSET_REFS)
    }

    /// `Archetype.HasSystemInstanceComponents` — src: `get_HasSystemInstanceComponents_11`, bit 2048.
    #[inline]
    pub const fn has_system_instance_components(&self) -> bool {
        self.data
            .flags
            .contains(ArchetypeFlags::HAS_SYSTEM_INSTANCE_COMPONENTS)
    }

    /// `Archetype.HasChunkComponents` — src: `get_HasChunkComponents_25`:
    /// `FirstChunkComponent != TypesCount` (@ 0x911c4e0 compiles to `cmp; setne`, not `> 0`; the
    /// two only differ when `TypesCount < FirstChunkComponent`, which the original replay oracle
    /// `tests/oracle_entities.rs` exercises).
    #[inline]
    pub const fn has_chunk_components(&self) -> bool {
        self.types_count != self.data.layout.first_chunk_component
    }

    /// `Archetype.IsManaged` — src: `IsManaged_26` @ 0x911c340: the managed, companion and
    /// system-instance bits ORed together.
    #[inline]
    pub const fn is_managed(&self) -> bool {
        self.data
            .flags
            .contains(ArchetypeFlags::HAS_MANAGED_COMPONENTS)
            || self
                .data
                .flags
                .contains(ArchetypeFlags::HAS_COMPANION_COMPONENTS)
            || self
                .data
                .flags
                .contains(ArchetypeFlags::HAS_SYSTEM_INSTANCE_COMPONENTS)
    }

    /// `Archetype.CompareMask` — src: `CompareMask_35` @ 0x911cc50:
    /// `*(byte*)(this + *maskPtr + 0x160) & maskPtr[1] == maskPtr[1]`. A mask pointer is a
    /// `(byteIndex, bit)` pair, which is what `EntityQueryMask` hands the archetype list.
    #[inline]
    pub fn compare_mask(&self, byte_index: usize, bit: u8) -> bool {
        byte_index < K_QUERY_MASK_BYTES && self.query_mask_array[byte_index] & bit == bit
    }

    /// `Archetype.SetMask` — src: `SetMask_36` @ 0x911cc70:
    /// `p = this + *maskPtr + 0x160; *p |= maskPtr[1]`.
    #[inline]
    pub fn set_mask(&mut self, byte_index: usize, bit: u8) {
        if byte_index < K_QUERY_MASK_BYTES {
            self.query_mask_array[byte_index] |= bit;
        }
    }

    /// Clear one query's bit, the inverse of `SetMask`, used when a query is destroyed.
    #[inline]
    pub fn clear_mask(&mut self, byte_index: usize, bit: u8) {
        if byte_index < K_QUERY_MASK_BYTES {
            self.query_mask_array[byte_index] &= !bit;
        }
    }

    /// `Archetype.AddToChunkList` — src: @ 0x911c660. Appends the chunk and returns the new position,
    /// which is what the body writes into the per-chunk back-reference array at `+0xc`.
    pub fn add_to_chunk_list(&mut self, chunk_index: i32) -> usize {
        let position = self.chunks.len();
        self.chunks.push(chunk_index);
        position
    }

    /// `Archetype.RemoveFromChunkList` — src: @ 0x911c7d0. Swap-back removal: the last chunk moves
    /// into the vacated slot. Returns the chunk that moved together with the position it moved to, so
    /// the caller can rewrite that chunk's stored back-reference.
    pub fn remove_from_chunk_list(&mut self, position: usize) -> Option<(i32, usize)> {
        if position >= self.chunks.len() {
            return None;
        }
        let moved = self.chunks.swap_remove(position);
        Some((moved, position))
    }

    /// `Archetype.EmptySlotTrackingAddChunk` — src: @ 0x911c9b0. Records the chunk's position in the
    /// archetype's chunk list on `ChunksWithEmptySlots`.
    pub fn empty_slot_tracking_add_chunk(&mut self, index_in_archetype_list: i32) {
        self.chunks_with_empty_slots.push(index_in_archetype_list);
    }

    /// `Archetype.EmptySlotTrackingRemoveChunk` — src: @ 0x911c940. Swap-removes the recorded
    /// position. Returns the position that was displaced, if any, so the displaced chunk can be told
    /// its new home.
    pub fn empty_slot_tracking_remove_chunk(&mut self, position_in_empty_list: i32) -> Option<i32> {
        if position_in_empty_list < 0
            || (position_in_empty_list as usize) >= self.chunks_with_empty_slots.len()
        {
            return None;
        }
        let displaced = self
            .chunks_with_empty_slots
            .swap_remove(position_in_empty_list as usize);
        if (position_in_empty_list as usize) < self.chunks_with_empty_slots.len() {
            Some(displaced)
        } else {
            None
        }
    }

    /// `Archetype.GetExistingChunkWithEmptySlots` — src: @ 0x911ca20. The last recorded chunk with
    /// room, or [`NONE`].
    #[inline]
    pub fn get_existing_chunk_with_empty_slots(&self) -> i32 {
        self.chunks_with_empty_slots
            .last()
            .and_then(|&pos| self.chunks.get(pos as usize).copied())
            .unwrap_or(NONE)
    }

    /// `Archetype.AddToChunkListWithEmptySlots` — src: @ 0x911c870. The original allocates a node in
    /// an intrusive list and stores the chunk's `ListWithEmptySlotsIndex` in it; the port keeps the
    /// position in `ChunksWith_empty_slots` and the intrusive link on the chunk.
    #[inline]
    pub fn add_to_chunk_list_with_empty_slots(&mut self, index_in_archetype_list: i32) {
        self.empty_slot_tracking_add_chunk(index_in_archetype_list);
    }

    /// `Archetype.RemoveFromChunkListWithEmptySlots` — src: @ 0x911c8f0.
    #[inline]
    pub fn remove_from_chunk_list_with_empty_slots(&mut self, position_in_empty_list: i32) {
        self.empty_slot_tracking_remove_chunk(position_in_empty_list);
    }

    /// Is `type_index` in this archetype's type list? Returns its index in the archetype.
    ///
    /// The original answers this from the archetype's bloom filter plus a scan of `Types`
    /// (`ChunkDataUtility.FindArchetypeIndex`); the bloom test can only say "definitely not present",
    /// so the scan is the answer either way.
    pub fn index_of_type(&self, type_index: TypeIndex) -> Option<usize> {
        self.types
            .iter()
            .position(|t| t.type_index.value == type_index.value)
    }

    /// The `ComponentTypeInArchetype` for a type, if the archetype has it.
    #[inline]
    pub fn type_at(&self, type_index_in_archetype: usize) -> Option<&ComponentTypeInArchetype> {
        self.types.get(type_index_in_archetype)
    }

    /// The archetype's bloom filter mask, `Archetype.BloomFilterMask` @ 0xe8 — the OR of the
    /// per-type masks CreateArchetype accumulates at 0x8fb1267 (`uVar26 |= *(ulonglong*)(TypeInfo +
    /// 0x20)`). A query can reject a whole archetype with one test against it.
    #[inline]
    pub const fn bloom_filter_mask(&self) -> u64 {
        self.data.bloom_filter_mask
    }
}
