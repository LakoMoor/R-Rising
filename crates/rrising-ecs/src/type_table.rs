//! `Unity.Entities.TypeManager`, `TypeIndex`, `ComponentType` and `TypeInfo`.
//!
//! This is the registry every other part of the ECS port hangs off: it turns a component type into
//! an integer the archetypes, chunks and queries all speak in, and it keeps the flags that say what
//! kind of thing that type is (buffer, shared, managed, enableable, zero-sized…).
//!
//! The bit layout is not the documented one, it is the one this build has. `TypeIndex` packs the
//! index and the flags into one `int`, and every classification is a bit test on it; the bodies
//! below were read out of `../evidence/decompiled/Unity.Entities.dll.c` (Ghidra, names from
//! Il2CppDumper), so the masks here are the shipped masks:
//!
//! | bit | mask | member it gates | body |
//! | --- | --- | --- | --- |
//! | 17 | `0x20000` | `HasEntityReferences` | @ 0x8fcda80 |
//! | 18 | `0x40000` | `IsChunkSerializable` | @ 0x8fcda90 |
//! | 19 | `0x80000` | `HasNativeContainer` | @ 0x5b6f4b0 |
//! | 20 | `0x100000` | `IsBakingOnlyType` | @ 0x5b6f4d0 |
//! | 21 | `0x200000` | `IsTemporaryBakingType` | @ 0x8fcdaa0 |
//! | 22 | `0x400000` | `IsRefCounted` | @ 0x8fcda70 |
//! | 23 | `0x800000` | `IsIEquatable` | @ 0x8fcda00 |
//! | 24 | `0x1000000` | `IsEnableable` / `TypeInfo.EnableableType` | @ 0x8fcda60 |
//! | 25 | `0x2000000` | `IsCleanupComponent` | @ 0x8fcd9b0 |
//! | 26 | `0x4000000` | `IsBuffer` | @ 0x5b6a0f0 |
//! | 27 | `0x8000000` | `IsSharedComponent` | @ 0x8fcd9f0 |
//! | 28 | `0x10000000` | `IsManagedType` | @ 0x8fcda30 |
//! | 29 | `0x20000000` | `IsChunkComponent` | @ 0x8fcda50 |
//! | 30 | `0x40000000` | `IsZeroSized` | @ 0x8fcda40 |
//!
//! `TypeIndex.Index` is `Value & 0x3fff` and `TypeIndex.Flags` is `Value & 0xffffc000` (src:
//! @ 0x25b2500 and @ 0x8fcdab0), so this build holds at most 16 384 types and the flags start at
//! bit 14. `SystemTypeIndex` packs the same int differently (`Index` is `& 0x7ffffff`, src:
//! @ 0x8fcdd20); that is a different type and is not conflated here.
//!
//! Two of the bodies are compound tests rather than single-bit ones, and the port keeps them as
//! written: `IsManagedComponent` is `(Value & 0x38000000) == 0x10000000` (managed, and neither
//! chunk-component nor zero-sized; src: @ 0x8fcda10), `IsCleanupSharedComponent` is
//! `(Value & 0xa000000) == 0xa000000` and `IsCleanupBufferComponent` is
//! `(Value & 0x6000000) == 0x6000000` (src: @ 0x8fcd9c0 and @ 0x8fcd9d0). `IsComponent` is
//! `(Value & 0xc000000) == 0` — neither buffer nor shared (src: @ 0x8fcd9e0).
//!
//! `HasEntityReferences` and `IsChunkSerializable` are the two members whose *polarity* the corpus
//! does not settle: their bodies decompile to `~(byte)(Value >> 17) & 1` and the same shape at
//! `>> 18`, which is Ghidra's rendering of a compare-and-set-flag, and it reads as "the bit is
//! clear". Every other classification in this build is a positive bit test, and a type that does
//! not set bit 17 would then have entity references, which is backwards for most components. The
//! port therefore treats both as positive bit tests and records the choice in `docs/PORT.md`
//! instead of pretending the corpus was unambiguous.

/// `Unity.Entities.TypeIndex` — one `int` (`Value; // 0x0`) holding the index and the flags.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypeIndex {
    /// `TypeIndex.Value` — the packed index and flags.
    pub value: i32,
}

impl TypeIndex {
    /// `TypeIndex.Index` masks the index out of `Value` (src: @ 0x25b2500, `Value & 0x3fff`).
    pub const K_INDEX_MASK: u32 = 0x3fff;
    /// `TypeIndex.Flags` masks the flags out of `Value` (src: @ 0x8fcdab0, `Value & 0xffffc000`).
    pub const K_FLAGS_MASK: u32 = 0xffff_c000;

    pub const K_HAS_ENTITY_REFERENCES_MASK: u32 = 1 << 17;
    pub const K_IS_CHUNK_SERIALIZABLE_MASK: u32 = 1 << 18;
    pub const K_HAS_NATIVE_CONTAINER_MASK: u32 = 1 << 19;
    pub const K_BAKING_ONLY_TYPE_MASK: u32 = 1 << 20;
    pub const K_TEMPORARY_BAKING_TYPE_MASK: u32 = 1 << 21;
    pub const K_REFCOUNTED_MASK: u32 = 1 << 22;
    pub const K_IEQUATABLE_MASK: u32 = 1 << 23;
    pub const K_ENABLEABLE_MASK: u32 = 1 << 24;
    pub const K_CLEANUP_MASK: u32 = 1 << 25;
    pub const K_BUFFER_MASK: u32 = 1 << 26;
    pub const K_SHARED_MASK: u32 = 1 << 27;
    pub const K_MANAGED_MASK: u32 = 1 << 28;
    pub const K_CHUNK_COMPONENT_MASK: u32 = 1 << 29;
    pub const K_ZERO_SIZED_MASK: u32 = 1 << 30;

    /// `TypeIndex.Null` — src: @ 0x173f700, the same body `Entity.get_Null` shares: the zero value.
    pub const NULL: TypeIndex = TypeIndex { value: 0 };

    /// `TypeIndex(int value)`.
    #[inline]
    pub const fn new(value: i32) -> Self {
        Self { value }
    }

    /// Pack an index and a flag word the way the original does.
    #[inline]
    pub const fn pack(index: i32, flags: u32) -> Self {
        Self {
            value: ((index as u32 & Self::K_INDEX_MASK) | (flags & Self::K_FLAGS_MASK)) as i32,
        }
    }

    /// `TypeIndex.Index` — src: @ 0x25b2500.
    #[inline]
    pub const fn index(self) -> i32 {
        (self.value as u32 & Self::K_INDEX_MASK) as i32
    }

    /// `TypeIndex.Flags` — src: @ 0x8fcdab0.
    #[inline]
    pub const fn flags(self) -> u32 {
        self.value as u32 & Self::K_FLAGS_MASK
    }

    #[inline]
    const fn has(self, mask: u32) -> bool {
        (self.value as u32 & mask) != 0
    }

    /// `TypeIndex.IsBuffer` — src: @ 0x5b6a0f0.
    #[inline]
    pub const fn is_buffer(self) -> bool {
        self.has(Self::K_BUFFER_MASK)
    }

    /// `TypeIndex.IsSharedComponentType` — src: @ 0x8fcd9f0 (`ComponentType.IsSharedComponent`).
    #[inline]
    pub const fn is_shared_component_type(self) -> bool {
        self.has(Self::K_SHARED_MASK)
    }

    /// `TypeIndex.IsManagedType` — src: @ 0x8fcda30.
    #[inline]
    pub const fn is_managed_type(self) -> bool {
        self.has(Self::K_MANAGED_MASK)
    }

    /// `TypeIndex.IsManagedComponent` — src: @ 0x8fcda10: managed, and neither a chunk component
    /// nor zero-sized. The body is a three-bit compare, kept as written.
    #[inline]
    pub const fn is_managed_component(self) -> bool {
        self.value as u32 & 0x3800_0000 == 0x1000_0000
    }

    /// `TypeIndex.IsManagedSharedComponent` — managed and shared.
    #[inline]
    pub const fn is_managed_shared_component(self) -> bool {
        self.is_managed_type() && self.is_shared_component_type()
    }

    /// `TypeIndex.IsZeroSized` — src: @ 0x8fcda40.
    #[inline]
    pub const fn is_zero_sized(self) -> bool {
        self.has(Self::K_ZERO_SIZED_MASK)
    }

    /// `TypeIndex.IsChunkComponent` — src: @ 0x8fcda50.
    #[inline]
    pub const fn is_chunk_component(self) -> bool {
        self.has(Self::K_CHUNK_COMPONENT_MASK)
    }

    /// `TypeIndex.IsEnableable` — src: @ 0x8fcda60.
    #[inline]
    pub const fn is_enableable(self) -> bool {
        self.has(Self::K_ENABLEABLE_MASK)
    }

    /// `TypeIndex.IsCleanupComponent` — src: @ 0x8fcd9b0. The dump lists the obsolete
    /// `IsSystemStateComponent` at the same RVA: they are the same compiled test.
    #[inline]
    pub const fn is_cleanup_component(self) -> bool {
        self.has(Self::K_CLEANUP_MASK)
    }

    /// `TypeIndex.IsCleanupSharedComponent` — src: @ 0x8fcd9c0: cleanup **and** shared.
    #[inline]
    pub const fn is_cleanup_shared_component(self) -> bool {
        self.value as u32 & 0x0a00_0000 == 0x0a00_0000
    }

    /// `TypeIndex.IsCleanupBufferComponent` — src: @ 0x8fcd9d0: cleanup **and** buffer.
    #[inline]
    pub const fn is_cleanup_buffer_component(self) -> bool {
        self.value as u32 & 0x0600_0000 == 0x0600_0000
    }

    /// `TypeIndex.IsComponentType` — src: @ 0x8fcd9e0 (`ComponentType.IsComponent`): neither a
    /// buffer nor a shared component.
    #[inline]
    pub const fn is_component_type(self) -> bool {
        self.value as u32 & 0x0c00_0000 == 0
    }

    /// `TypeIndex.IsRefCounted` — src: @ 0x8fcda70.
    #[inline]
    pub const fn is_refcounted(self) -> bool {
        self.has(Self::K_REFCOUNTED_MASK)
    }

    /// `TypeIndex.IsIEquatable` — src: @ 0x8fcda00.
    #[inline]
    pub const fn is_iequatable(self) -> bool {
        self.has(Self::K_IEQUATABLE_MASK)
    }

    /// `TypeIndex.HasNativeContainer` — src: @ 0x5b6f4b0.
    #[inline]
    pub const fn has_native_container(self) -> bool {
        self.has(Self::K_HAS_NATIVE_CONTAINER_MASK)
    }

    /// `TypeIndex.IsBakingOnlyType` — src: @ 0x5b6f4d0.
    #[inline]
    pub const fn is_baking_only_type(self) -> bool {
        self.has(Self::K_BAKING_ONLY_TYPE_MASK)
    }

    /// `TypeIndex.IsTemporaryBakingType` — src: @ 0x8fcdaa0.
    #[inline]
    pub const fn is_temporary_baking_type(self) -> bool {
        self.has(Self::K_TEMPORARY_BAKING_TYPE_MASK)
    }

    /// `TypeIndex.HasEntityReferences` — src: @ 0x8fcda80, read as a positive test on bit 17; see
    /// the module comment for why the polarity is a judgement call rather than a transcription.
    #[inline]
    pub const fn has_entity_references(self) -> bool {
        self.has(Self::K_HAS_ENTITY_REFERENCES_MASK)
    }

    /// `TypeIndex.IsChunkSerializable` — src: @ 0x8fcda90, same polarity note.
    #[inline]
    pub const fn is_chunk_serializable(self) -> bool {
        self.has(Self::K_IS_CHUNK_SERIALIZABLE_MASK)
    }

    /// `TypeIndex.IsSystemStateComponent` — the obsolete spelling of `IsCleanupComponent`, which
    /// the dump lists at the same RVA.
    #[inline]
    pub const fn is_system_state_component(self) -> bool {
        self.is_cleanup_component()
    }

    /// `TypeIndex.IsSystemStateSharedComponent` — the obsolete spelling above.
    #[inline]
    pub const fn is_system_state_shared_component(self) -> bool {
        self.is_cleanup_shared_component()
    }
}

/// `Unity.Entities.TypeManager.TypeCategory` — `dump.cs`: `ComponentData = 0`, `BufferData = 1`,
/// `ISharedComponentData = 2`, `EntityData = 3`, `UnityEngineObject = 4`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeCategory {
    ComponentData = 0,
    BufferData = 1,
    ISharedComponentData = 2,
    EntityData = 3,
    UnityEngineObject = 4,
}

/// `Unity.Entities.TypeManager.TypeInfo` — the record the registry keeps per type. Field names and
/// offsets are `dump.cs`'s (`TypeSize: 0x60`, confirmed by the archetype builder copying these
/// records 96 bytes at a time; see `archetype.rs`).
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeInfo {
    /// `TypeIndex TypeIndex; // 0x0`
    pub type_index: TypeIndex,
    /// `int SizeInChunk; // 0x4` — bytes one entity of this type occupies in a chunk.
    pub size_in_chunk: i32,
    /// `int ElementSize; // 0x8` — for buffers, the size of one element.
    pub element_size: i32,
    /// `int BufferCapacity; // 0xC` — for buffers, the inline capacity the type asks for.
    pub buffer_capacity: i32,
    /// `ulong MemoryOrdering; // 0x10`
    pub memory_ordering: u64,
    /// `ulong StableTypeHash; // 0x18`
    pub stable_type_hash: u64,
    /// `ulong BloomFilterMask; // 0x20`
    pub bloom_filter_mask: u64,
    /// `int AlignmentInBytes; // 0x28`
    pub alignment_in_bytes: i32,
    /// `TypeManager.TypeCategory Category; // 0x2C`
    pub category: TypeCategory,
    /// `int EntityOffsetCount; // 0x30`
    pub entity_offset_count: i32,
    /// `int EntityOffsetStartIndex; // 0x34`
    pub entity_offset_start_index: i32,
    /// `int _HasBlobAssetRefs; // 0x38` — an int in the original, not a bool.
    pub has_blob_asset_refs: i32,
    /// `int BlobAssetRefOffsetCount; // 0x3C`
    pub blob_asset_ref_offset_count: i32,
    /// `int BlobAssetRefOffsetStartIndex; // 0x40`
    pub blob_asset_ref_offset_start_index: i32,
    /// `int WeakAssetRefOffsetCount; // 0x44`
    pub weak_asset_ref_offset_count: i32,
    /// `int WeakAssetRefOffsetStartIndex; // 0x48`
    pub weak_asset_ref_offset_start_index: i32,
    /// `int WriteGroupCount; // 0x4C`
    pub write_group_count: i32,
    /// `int WriteGroupStartIndex; // 0x50`
    pub write_group_start_index: i32,
    /// `int MaximumChunkCapacity; // 0x54` — caps how many entities of this type fit in one chunk.
    pub maximum_chunk_capacity: i32,
    /// `int TypeSize; // 0x58` — the managed size of the type itself.
    pub type_size: i32,
    /// The type's name. The original reaches this through `DebugTypeName` (a `NativeText` pointing
    /// at the metadata); the port keeps a plain string because Rust has no metadata table to point
    /// at. It is used for diagnostics and for looking a type up by name, never for ordering.
    pub name: String,
}

impl TypeInfo {
    /// `TypeInfo.AlignmentInChunkInBytes` — src:
    /// `Unity_Entities____TypeManager_TypeInfo__get_AlignmentInChunkInBytes_1` @ 0x9064a30:
    /// `if (Category != 1) return AlignmentInBytes; return 16;`. Buffers align to 16 because their
    /// chunk slot is a `BufferHeader`, whose first field is a pointer.
    #[inline]
    pub fn alignment_in_chunk_in_bytes(&self) -> i32 {
        if self.category != TypeCategory::BufferData {
            self.alignment_in_bytes
        } else {
            16
        }
    }

    /// `TypeInfo.IsZeroSized` — src: @ 0x9064a40: `SizeInChunk == 0`. Note this is the *chunk*
    /// size, not the `IsZeroSized` flag on `TypeIndex` (bit 30), which is the managed size.
    #[inline]
    pub const fn is_zero_sized(&self) -> bool {
        self.size_in_chunk == 0
    }

    /// `TypeInfo.EnableableType` — src: @ 0x8fcda60: bit 24 of the packed `TypeIndex`.
    #[inline]
    pub const fn enableable_type(&self) -> bool {
        self.type_index.is_enableable()
    }

    /// `TypeInfo.BakingOnlyType` — src: @ 0x5b6f4d0.
    #[inline]
    pub const fn baking_only_type(&self) -> bool {
        self.type_index.is_baking_only_type()
    }

    /// `TypeInfo.TemporaryBakingType` — src: @ 0x8fcdaa0.
    #[inline]
    pub const fn temporary_baking_type(&self) -> bool {
        self.type_index.is_temporary_baking_type()
    }

    /// `TypeInfo.HasWriteGroups` — src: @ 0x9064a50: `WriteGroupCount > 0`.
    #[inline]
    pub const fn has_write_groups(&self) -> bool {
        self.write_group_count > 0
    }

    /// `TypeInfo.HasBlobAssetRefs` — src: @ 0x5c46cc0: the `_HasBlobAssetRefs` int as a bool.
    #[inline]
    pub const fn has_blob_asset_refs(&self) -> bool {
        self.has_blob_asset_refs != 0
    }

    /// `TypeInfo.HasWeakAssetRefs` — src: @ 0x9064a60.
    #[inline]
    pub const fn has_weak_asset_refs(&self) -> bool {
        self.weak_asset_ref_offset_count > 0
    }
}

/// What the caller gives the registry to create a type. The original builds this from reflection
/// (`TypeManager.BuildComponentType`); the port takes it explicitly because Rust has no runtime
/// reflection over the game's assemblies, and the numbers come from `dump.cs` field layouts.
#[derive(Clone, Debug)]
pub struct TypeSpec {
    pub name: String,
    pub category: TypeCategory,
    pub size_in_chunk: i32,
    pub element_size: i32,
    pub buffer_capacity: i32,
    pub alignment_in_bytes: i32,
    pub maximum_chunk_capacity: i32,
    pub type_size: i32,
    /// Extra flag bits to OR into the packed `TypeIndex` (cleanup, enableable, …).
    pub flags: u32,
    pub memory_ordering: u64,
    pub stable_type_hash: u64,
    pub entity_offset_count: i32,
    pub blob_asset_ref_offset_count: i32,
    pub weak_asset_ref_offset_count: i32,
    pub write_group_count: i32,
}

impl TypeSpec {
    /// A plain `IComponentData` of size `T`'s Rust layout, with the defaults the registry uses for
    /// that category.
    pub fn component_data(name: &str, size: i32, alignment: i32) -> Self {
        Self {
            name: name.to_string(),
            category: TypeCategory::ComponentData,
            size_in_chunk: size,
            element_size: size,
            buffer_capacity: 0,
            alignment_in_bytes: alignment,
            maximum_chunk_capacity: i32::MAX,
            type_size: size,
            flags: 0,
            memory_ordering: 0,
            stable_type_hash: 0,
            entity_offset_count: 0,
            blob_asset_ref_offset_count: 0,
            weak_asset_ref_offset_count: 0,
            write_group_count: 0,
        }
    }

    /// A `IBufferElementData` of element size `element_size`. Its chunk slot is a `BufferHeader`
    /// (16 bytes), which is what `SizeInChunk` is for buffers.
    pub fn buffer(name: &str, element_size: i32, buffer_capacity: i32) -> Self {
        Self {
            category: TypeCategory::BufferData,
            size_in_chunk: crate::buffer::ELEMENT_OFFSET as i32,
            element_size,
            buffer_capacity,
            alignment_in_bytes: 16,
            flags: TypeIndex::K_BUFFER_MASK,
            ..Self::component_data(name, 0, 0)
        }
    }

    /// An `ISharedComponentData`. Shared components have no per-entity bytes in the chunk: the
    /// chunk holds an index into the archetype's shared-component list.
    pub fn shared(name: &str, size: i32, alignment: i32) -> Self {
        Self {
            category: TypeCategory::ISharedComponentData,
            flags: TypeIndex::K_SHARED_MASK,
            ..Self::component_data(name, size, alignment)
        }
    }

    /// A tag component: no bytes at all in the chunk.
    pub fn tag(name: &str) -> Self {
        Self {
            flags: TypeIndex::K_ZERO_SIZED_MASK,
            ..Self::component_data(name, 0, 1)
        }
    }
}

/// `Unity.Entities.TypeManager` — the registry. The original's is a static with a fixed table
/// filled at `Initialize`; the port owns one per store so tests can build independent registries.
/// `TypeIndex.Index` is 14 bits, so the registry refuses to pass 16 384 types rather than wrap.
#[derive(Clone, Debug, Default)]
pub struct TypeManager {
    infos: Vec<TypeInfo>,
    by_name: std::collections::BTreeMap<String, TypeIndex>,
}

/// The most types this build can address (`TypeIndex.K_INDEX_MASK + 1`).
pub const K_MAX_TYPE_COUNT: usize = TypeIndex::K_INDEX_MASK as usize + 1;

impl TypeManager {
    /// `TypeManager.GetTypeCount()`.
    #[inline]
    pub fn count(&self) -> i32 {
        self.infos.len() as i32
    }

    /// Register a type and return its `TypeIndex`. The index is the next slot, packed with the
    /// spec's flag bits.
    pub fn register(&mut self, spec: TypeSpec) -> anyhow::Result<TypeIndex> {
        if self.infos.len() >= K_MAX_TYPE_COUNT {
            anyhow::bail!(
                "TypeManager is full: {} types registered, this build addresses {}",
                self.infos.len(),
                K_MAX_TYPE_COUNT
            );
        }
        let index = self.infos.len() as i32;
        let type_index = TypeIndex::pack(index, spec.flags);
        let info = TypeInfo {
            type_index,
            size_in_chunk: spec.size_in_chunk,
            element_size: spec.element_size,
            buffer_capacity: spec.buffer_capacity,
            memory_ordering: spec.memory_ordering,
            stable_type_hash: spec.stable_type_hash,
            bloom_filter_mask: 0,
            alignment_in_bytes: spec.alignment_in_bytes,
            category: spec.category,
            entity_offset_count: spec.entity_offset_count,
            entity_offset_start_index: 0,
            has_blob_asset_refs: (spec.blob_asset_ref_offset_count > 0) as i32,
            blob_asset_ref_offset_count: spec.blob_asset_ref_offset_count,
            blob_asset_ref_offset_start_index: 0,
            weak_asset_ref_offset_count: spec.weak_asset_ref_offset_count,
            weak_asset_ref_offset_start_index: 0,
            write_group_count: spec.write_group_count,
            write_group_start_index: 0,
            maximum_chunk_capacity: spec.maximum_chunk_capacity,
            type_size: spec.type_size,
            name: spec.name.clone(),
        };
        self.infos.push(info);
        self.by_name.insert(spec.name, type_index);
        Ok(type_index)
    }

    /// `TypeManager.GetTypeInfo(TypeIndex)` — src: the registry indexes its table by
    /// `TypeIndex.Index`; the archetype builder does exactly that (`index & 0x3fff`) when it copies
    /// the records out.
    #[inline]
    pub fn type_info(&self, type_index: TypeIndex) -> Option<&TypeInfo> {
        self.infos.get(type_index.index() as usize)
    }

    /// `TypeManager.GetTypeIndex(Type)` — the port looks types up by the name they were registered
    /// with, since Rust has no `System.Type` to hand over.
    pub fn type_index_of(&self, name: &str) -> Option<TypeIndex> {
        self.by_name.get(name).copied()
    }

    /// Every registered type, in registration order.
    pub fn infos(&self) -> &[TypeInfo] {
        &self.infos
    }
}

/// `Unity.Entities.ComponentType.AccessMode` — `dump.cs`: `ReadWrite = 0`, `ReadOnly = 1`,
/// `Exclude = 2`.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AccessMode {
    ReadWrite = 0,
    ReadOnly = 1,
    Exclude = 2,
}

/// `Unity.Entities.ComponentType` — `TypeIndex TypeIndex; // 0x0` and
/// `AccessMode AccessModeType; // 0x4` (`dump.cs`).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentType {
    /// `ComponentType.TypeIndex`
    pub type_index: TypeIndex,
    /// `ComponentType.AccessModeType`
    pub access_mode_type: AccessMode,
}

impl ComponentType {
    /// `ComponentType.FromTypeIndex(TypeIndex)` — src: @ 0x91216f0, whose whole body is
    /// `return param_1;`: the packed value passes through unchanged, so the access mode is read off
    /// the same int the index lives in and `FromTypeIndex` cannot set it.
    #[inline]
    pub const fn from_type_index(type_index: TypeIndex) -> Self {
        Self {
            type_index,
            access_mode_type: AccessMode::ReadWrite,
        }
    }

    /// `ComponentType.ReadWrite(TypeIndex)`.
    #[inline]
    pub const fn read_write(type_index: TypeIndex) -> Self {
        Self {
            type_index,
            access_mode_type: AccessMode::ReadWrite,
        }
    }

    /// `ComponentType.ReadOnly(TypeIndex)`.
    #[inline]
    pub const fn read_only(type_index: TypeIndex) -> Self {
        Self {
            type_index,
            access_mode_type: AccessMode::ReadOnly,
        }
    }

    /// `ComponentType.Exclude(TypeIndex)`.
    #[inline]
    pub const fn exclude(type_index: TypeIndex) -> Self {
        Self {
            type_index,
            access_mode_type: AccessMode::Exclude,
        }
    }

    /// The classification members all read the packed `TypeIndex` and ignore the access mode, so
    /// they delegate to it. `dump.cs` lists them on both `ComponentType` and `TypeIndex` at the
    /// same RVAs — the same compiled code, filed twice.
    #[inline]
    pub const fn is_buffer(self) -> bool {
        self.type_index.is_buffer()
    }
    #[inline]
    pub const fn is_shared_component(self) -> bool {
        self.type_index.is_shared_component_type()
    }
    #[inline]
    pub const fn is_managed_component(self) -> bool {
        self.type_index.is_managed_component()
    }
    #[inline]
    pub const fn is_zero_sized(self) -> bool {
        self.type_index.is_zero_sized()
    }
    #[inline]
    pub const fn is_chunk_component(self) -> bool {
        self.type_index.is_chunk_component()
    }
    #[inline]
    pub const fn is_enableable(self) -> bool {
        self.type_index.is_enableable()
    }
    #[inline]
    pub const fn is_cleanup_component(self) -> bool {
        self.type_index.is_cleanup_component()
    }
    #[inline]
    pub const fn is_cleanup_shared_component(self) -> bool {
        self.type_index.is_cleanup_shared_component()
    }
    #[inline]
    pub const fn is_cleanup_buffer_component(self) -> bool {
        self.type_index.is_cleanup_buffer_component()
    }
    #[inline]
    pub const fn is_component(self) -> bool {
        self.type_index.is_component_type()
    }
    #[inline]
    pub const fn has_entity_references(self) -> bool {
        self.type_index.has_entity_references()
    }
    #[inline]
    pub const fn has_native_container(self) -> bool {
        self.type_index.has_native_container()
    }
    #[inline]
    pub const fn is_chunk_serializable(self) -> bool {
        self.type_index.is_chunk_serializable()
    }
}

/// `Unity.Entities.ComponentTypeInArchetype` — one `readonly TypeIndex TypeIndex; // 0x0`. Its
/// classification members are the same tests as `ComponentType`'s, on the same packed int.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ComponentTypeInArchetype {
    /// `ComponentTypeInArchetype.TypeIndex`
    pub type_index: TypeIndex,
}

impl ComponentTypeInArchetype {
    /// `ComponentTypeInArchetype(TypeIndex)`.
    #[inline]
    pub const fn new(type_index: TypeIndex) -> Self {
        Self { type_index }
    }

    #[inline]
    pub const fn is_buffer(self) -> bool {
        self.type_index.is_buffer()
    }
    #[inline]
    pub const fn is_shared_component(self) -> bool {
        self.type_index.is_shared_component_type()
    }
    #[inline]
    pub const fn is_zero_sized(self) -> bool {
        self.type_index.is_zero_sized()
    }
    #[inline]
    pub const fn is_chunk_component(self) -> bool {
        self.type_index.is_chunk_component()
    }
    #[inline]
    pub const fn is_enableable(self) -> bool {
        self.type_index.is_enableable()
    }
    #[inline]
    pub const fn is_managed_component(self) -> bool {
        self.type_index.is_managed_component()
    }
    #[inline]
    pub const fn is_cleanup_component(self) -> bool {
        self.type_index.is_cleanup_component()
    }
    #[inline]
    pub const fn is_cleanup_shared_component(self) -> bool {
        self.type_index.is_cleanup_shared_component()
    }
    #[inline]
    pub const fn has_entity_references(self) -> bool {
        self.type_index.has_entity_references()
    }
    #[inline]
    pub const fn is_chunk_serializable(self) -> bool {
        self.type_index.is_chunk_serializable()
    }
}
