//! `Unity.Entities.Chunk` and `Unity.Entities.ChunkIndex` — the 32 KiB block that actually holds
//! component bytes, and the handle the store uses to name one.
//!
//! src: `dump.cs`, `internal struct Chunk // TypeDefIndex: 24058` and
//! `internal struct ChunkIndex : IComparable<ChunkIndex> // TypeDefIndex: 24056`, plus the constants
//! those two types declare. The corpus has no decompiled body for `Chunk`'s own members (the store
//! reaches the bytes through `ChunkIndex.GetPtr` @ 0x91212b0, whose body is not in the focus set
//! either), so what is ported here is the *layout* — which is what the entity-scene reader and every
//! component accessor depend on — and the accessors are named after the `ChunkIndex` properties in
//! `dump.cs` rather than invented bodies.
//!
//! The substitution, recorded in `docs/PORT.md`: the original addresses a chunk through a
//! megachunk/universe scheme (`ChunkIndex.Value` is an index into a flat address space sized by
//! `log2ChunksPerMegachunk` and `log2MegachunksInUniverse`, and `ArchetypeChunkData` keeps parallel
//! arrays of change versions and shared-component values alongside it). The port keeps a plain `Vec`
//! of 32 KiB blocks, index 0 reserved as the null chunk, so `ChunkIndex.Value` is just that index.
//! The observable behaviour — "give me this chunk's bytes, count, flags, sequence number" — is the
//! same; only the address arithmetic behind `Value` differs.

/// `Chunk.kChunkSize = 32768` — the size of one chunk allocation.
pub const K_CHUNK_SIZE: usize = 32768;

/// `Chunk.kBufferOffset = 64` — the component bytes start here; everything below is the header.
pub const K_BUFFER_OFFSET: usize = 64;

/// `Chunk.kBufferSize = 32704` / `Chunk.kChunkBufferSize = 32704` — the usable bytes after the
/// header. `Archetype.Create` fits every type's block inside this.
pub const K_BUFFER_SIZE: usize = 32704;

/// `Chunk.kSerializedHeaderSize = 40` — how much of the header an entity-scene file writes. The
/// remaining 24 bytes up to `kBufferOffset` are padding.
pub const K_SERIALIZED_HEADER_SIZE: usize = 40;

/// `Chunk.kMaximumEntitiesPerChunk = 4088` — the ceiling `Archetype.Create` asserts against
/// (`Assert__IsTrue_2(iVar20 < 0xff9)` @ 0x8fb173f). 4088 is `32704 / 8`, the smallest per-entity
/// stride a non-zero-sized component can have.
pub const K_MAXIMUM_ENTITIES_PER_CHUNK: i32 = 4088;

/// `ChunkFlags` — `dump.cs`: `internal enum ChunkFlags`, backed by `int`. Only four values exist in
/// this build; the two low bits are marked unused and the third is a temporary assert flag.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkFlags(pub i32);

impl ChunkFlags {
    /// `ChunkFlags.None = 0`
    pub const NONE: Self = Self(0);
    /// `ChunkFlags.Unused0 = 1`
    pub const UNUSED_0: Self = Self(1);
    /// `ChunkFlags.Unused1 = 2`
    pub const UNUSED_1: Self = Self(2);
    /// `ChunkFlags.TempAssertWillDestroyAllInLinkedEntityGroup = 4`
    pub const TEMP_ASSERT_WILL_DESTROY_ALL_IN_LINKED_ENTITY_GROUP: Self = Self(4);

    #[inline]
    pub const fn contains(self, bit: Self) -> bool {
        self.0 & bit.0 != 0
    }

    #[inline]
    pub const fn with(self, bit: Self) -> Self {
        Self(self.0 | bit.0)
    }
}

/// `Unity.Entities.ChunkIndex` — `private int Value; // 0x0`, the handle the store passes around
/// instead of a `Chunk*`.
///
/// The properties below are `ChunkIndex`'s (`dump.cs`); each one reads or writes one field of the
/// chunk header at the offset `Chunk` declares.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkIndex {
    /// `ChunkIndex.Value`
    pub value: i32,
}

impl ChunkIndex {
    /// `ChunkIndex.Null` — src: `get_Null` @ 0x173f700. The store's chunk array reserves index 0, so
    /// null is 0.
    pub const NULL: Self = Self { value: 0 };

    /// `ChunkIndex(int value)` — src: `.ctor` @ 0x17d85c0.
    #[inline]
    pub const fn new(value: i32) -> Self {
        Self { value }
    }

    /// `ChunkIndex.op_Implicit(int)` / the reverse — src: @ 0x17d86f0.
    #[inline]
    pub const fn as_int(self) -> i32 {
        self.value
    }

    #[inline]
    pub const fn is_null(self) -> bool {
        self.value == 0
    }

    /// `ChunkIndex.op_Equality` — src: @ 0x17d8680.
    #[inline]
    pub const fn eq(a: Self, b: Self) -> bool {
        a.value == b.value
    }

    /// `ChunkIndex.op_LessThan` — src: @ 0x8fc dac0. IL2CPP merges identical bodies, so this address
    /// is shared with other `op_LessThan` overloads in the dump; the comparison is on `Value`.
    #[inline]
    pub const fn lt(a: Self, b: Self) -> bool {
        a.value < b.value
    }
}

/// The 40 bytes of chunk header an entity-scene file writes, `Chunk.kSerializedHeaderSize`.
///
/// src: `dump.cs`, `Chunk`'s fields in declaration order:
/// `int ArchetypeIndexForSerialization; // 0x0`, `Entity metaChunkEntity; // 0x8`,
/// `int CountForSerialization; // 0x10`, `int ListWithEmptySlotsIndex; // 0x1C`,
/// `uint Flags; // 0x20`, `ulong SequenceNumber; // 0x28`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChunkHeader {
    /// `int ArchetypeIndexForSerialization; // 0x0`
    pub archetype_index: i32,
    /// `Entity metaChunkEntity; // 0x8` — the singleton entity that owns this chunk's shared
    /// component values.
    pub meta_chunk_entity: crate::entity::Entity,
    /// `int CountForSerialization; // 0x10` — live entities in the chunk.
    pub count: i32,
    /// `int ListWithEmptySlotsIndex; // 0x1c` — position in the archetype's
    /// `ChunksWithEmptySlots`, or -1 when the chunk is full.
    pub list_with_empty_slots_index: i32,
    /// `uint Flags; // 0x20`
    pub flags: u32,
    /// `ulong SequenceNumber; // 0x28` — unique per chunk, handed out by
    /// `EntityComponentStore.m_NextChunkSequenceNumber` @ 0x188. `EntityDiffer` matches chunks
    /// across two worlds by this number.
    pub sequence_number: u64,
}

/// One 32 KiB chunk: the header fields `Chunk` declares, plus the component bytes.
///
/// The original's `Chunk` is the header followed by `byte[4] Buffer` at `0x40` — a fixed buffer that
/// stands for the whole rest of the allocation. The port keeps the same fields and a `Vec<u8>` of
/// `K_BUFFER_SIZE` bytes for the payload.
#[derive(Clone, Debug)]
pub struct Chunk {
    /// `int ArchetypeIndexForSerialization; // 0x0`
    pub archetype_index: i32,
    /// `Entity metaChunkEntity; // 0x8`
    pub meta_chunk_entity: crate::entity::Entity,
    /// `int CountForSerialization; // 0x10` — `ChunkIndex.Count`.
    pub count: i32,
    /// `int ListWithEmptySlotsIndex; // 0x1c` — `ChunkIndex.ListWithEmptySlotsIndex`.
    pub list_with_empty_slots_index: i32,
    /// `uint Flags; // 0x20` — `ChunkIndex.Flags`.
    pub flags: u32,
    /// `ChunkIndex.ListIndex` — the chunk's position in its archetype's `Chunks` list. The original
    /// keeps this in a parallel array inside `ArchetypeChunkData`; the port keeps it on the chunk.
    /// (`Archetype.AddToChunkList` @ 0x911c660 is what writes it.)
    pub list_index: i32,
    /// `ulong SequenceNumber; // 0x28` — `ChunkIndex.SequenceNumber`.
    pub sequence_number: u64,
    /// `Chunk.Buffer` at `0x40` — the component bytes, laid out by `ArchetypeLayout::offsets`.
    pub buffer: Vec<u8>,
}

impl Chunk {
    /// A fresh, empty chunk of one archetype with the given sequence number.
    pub fn new(archetype_index: i32, sequence_number: u64) -> Self {
        Self {
            archetype_index,
            meta_chunk_entity: crate::entity::Entity::NULL,
            count: 0,
            list_with_empty_slots_index: -1,
            flags: 0,
            sequence_number,
            list_index: -1,
            buffer: vec![0; K_BUFFER_SIZE],
        }
    }

    /// `ChunkIndex.Buffer` — the base address of the component bytes. In the original this is
    /// `(byte*)this + kBufferOffset`; here it is the start of `buffer`.
    #[inline]
    pub fn buffer_mut(&mut self) -> &mut [u8] {
        &mut self.buffer
    }

    #[inline]
    pub fn buffer(&self) -> &[u8] {
        &self.buffer
    }

    /// `ChunkIndex.Count` — entities in this chunk.
    #[inline]
    pub const fn count(&self) -> i32 {
        self.count
    }

    /// `ChunkIndex.SequenceNumber`.
    #[inline]
    pub const fn sequence_number(&self) -> u64 {
        self.sequence_number
    }

    /// `ChunkIndex.Flags` as the enum the store compares against.
    #[inline]
    pub const fn chunk_flags(&self) -> ChunkFlags {
        ChunkFlags(self.flags as i32)
    }

    /// Free slots left for this archetype's capacity.
    #[inline]
    pub fn empty_slots(&self, chunk_capacity: i32) -> i32 {
        chunk_capacity - self.count
    }

    /// Write the 40-byte serialized header of this chunk into `out`, in the order the entity-scene
    /// writer uses. src: `Chunk.kSerializedHeaderSize = 40` and the field offsets above.
    pub fn write_serialized_header(&self, out: &mut [u8]) {
        debug_assert!(out.len() >= K_SERIALIZED_HEADER_SIZE);
        out[0..4].copy_from_slice(&self.archetype_index.to_le_bytes());
        out[8..12].copy_from_slice(&self.meta_chunk_entity.index.to_le_bytes());
        out[12..16].copy_from_slice(&self.meta_chunk_entity.version.to_le_bytes());
        out[16..20].copy_from_slice(&self.count.to_le_bytes());
        out[28..32].copy_from_slice(&self.list_with_empty_slots_index.to_le_bytes());
        out[32..36].copy_from_slice(&self.flags.to_le_bytes());
        out[36..44].copy_from_slice(&self.sequence_number.to_le_bytes());
    }

    /// Read a 40-byte serialized header. src: the same field order.
    pub fn read_serialized_header(bytes: &[u8]) -> Option<ChunkHeader> {
        if bytes.len() < K_SERIALIZED_HEADER_SIZE {
            return None;
        }
        let g = |r: std::ops::Range<usize>| -> u64 {
            let mut acc = 0u64;
            for (i, &b) in bytes[r].iter().enumerate() {
                acc |= (b as u64) << (8 * i);
            }
            acc
        };
        Some(ChunkHeader {
            archetype_index: g(0..4) as i32,
            meta_chunk_entity: crate::entity::Entity::new(g(8..12) as i32, g(12..16) as i32),
            count: g(16..20) as i32,
            list_with_empty_slots_index: g(28..32) as i32,
            flags: g(32..36) as u32,
            sequence_number: g(36..44),
        })
    }
}
