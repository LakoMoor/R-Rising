//! `Unity.Entities.Serialization.DotsSerialization` — the container the shipped `.entities`
//! scene files use.
//!
//! Every file under `StreamingAssets/EntityScenes` starts with the same 152-byte
//! `DotsSerialization.FileHeader` (`dump.cs`, TypeDefIndex 24342): magic `DOTSBIN!`, version 77,
//! a `Hash128` file id, a fixed-size file-type string, and the offsets of three sections —
//! data, metadata and nodes. The nodes section is a tree of `DotsSerialization.NodeHeader`
//! records (TypeDefIndex 24344), and the data section holds the bytes those nodes point at.
//!
//! Everything here was read off the shipped files, not inferred from Unity's source, and the
//! offsets below are the ones the files actually use:
//!
//! | offset | field | verified by |
//! | --- | --- | --- |
//! | 0x00 | `i64 MagicValue` = `"DOTSBIN!"` | every `.entities` and every `.entityheader` |
//! | 0x08 | `i32 FileVersion` = 77 | ditto |
//! | 0x0c | `i32 HeaderSize` = 152 | ditto |
//! | 0x10 | `Hash128 FileId` | ditto |
//! | 0x20 | `FixedString64Bytes FileType` — `u16 length` then the bytes | `"EntityBinaryFile"` in every `.entities` |
//! | 0x60 | `i32 FirstLevelNodesCount` | ditto |
//! | 0x68 | `i64 NodesSectionOffset` | `header + data + metadata + nodes == file size` |
//! | 0x70 | `i32 NodesSectionSize` | ditto |
//! | 0x78 | `i64 MetadataSectionOffset` | ditto |
//! | 0x80 | `i32 MetadataSectionSize` | ditto |
//! | 0x88 | `i64 DataSectionOffset` | ditto |
//! | 0x90 | `i64 DataSectionSize` | ditto |
//!
//! The node-tree framing is the part the dump does not give away, and it is fixed by the files:
//! a node's children start in the bytes immediately after the parent, siblings are chained by
//! `NextSiblingOffset` counted **from the start of the nodes section** (`-1` ends the chain), and
//! `DataStartingOffset` is a plain file offset, not a section-relative one. In
//! `ddfc6a4db4009e2448b8f8f6f0965b8f.0.entities` the root folder node lists six children, they
//! occupy 72 + 6×80 = 552 bytes which is exactly `NodesSectionSize`, and their data blobs tile
//! the data section with nothing left over.
//!
//! What each blob *means* is settled, and it comes from the writer, not the file. Every child is
//! a `DotsSerialization.RevisionedRawDataNode` (`NodeHeader` plus `int Revision` at 0x48): the
//! three node types the dump lists with that shape are indistinguishable by size, but
//! `SerializeUtility..cctor` @ 0x8fef290 fills twelve `Hash128` statics through
//! `Hash128.StringToHash` @ 0x8fcb550, and each child's `Id` is one of them. So `NodeTypeHash`
//! says what the node *is* and `Id` says which stream it holds. `ddfc6a4db4009e2448b8f8b6f0965b8f
//! .0.entities` carries Archetypes, BlobAssets, SharedAndManagedComponents, EnabledBits, Chunks
//! and BufferData; the same six, in the same order, are in every other scene file sampled so far.
//!
//! The `Id` bytes are not the plain hex of those literals. `StringToHash(..., guidFormatted: true)`
//! reads each group of eight characters back to front, so on disk every byte of an `Id` is the
//! literal's byte with its two nibbles swapped — see `rrising_ecs::hash128` and
//! `tests/entity_scenes.rs`, which checks all twelve literals against the shipped files.

use std::path::Path;

use anyhow::{Context, Result, bail};

/// `DotsSerialization.FileHeader.MagicValue` — the eight bytes `DOTSBIN!`.
pub const MAGIC: u64 = 0x214e_4942_5354_4f44;
/// `DotsSerialization.FileHeader.FileVersion` in this build.
pub const FILE_VERSION: i32 = 77;
/// `DotsSerialization.FileHeader.HeaderSize` in this build.
pub const HEADER_SIZE: usize = 152;
/// `DotsSerialization.NodeHeader` — nine fields ending at 0x40, so 72 bytes.
pub const NODE_HEADER_SIZE: usize = 72;
/// The `NodeTypeHash` every root node in the shipped scenes carries. 72 bytes of node, so it is
/// the `FolderNode` the dump describes as `NodeHeader` and nothing else.
pub const NODE_TYPE_FOLDER: u64 = 0x587e_b6cb_58e1_c891;
/// The `NodeTypeHash` every child node of that root carries. 80 bytes of node, so `NodeHeader`
/// plus one `int` at 0x48.
pub const NODE_TYPE_RAW_DATA: u64 = 0x7ff5_f067_a875_f185;

/// `SerializeUtility.WorldNodeType` @ 0x10 — the tag on the root folder node.
///
/// The twelve tags below are the `Hash128` statics `SerializeUtility..cctor` @ 0x8fef290 fills
/// from string literals, written the way `Hash128.StringToHash` @ 0x8fcb550 leaves them on disk.
/// Each is paired with the literal it came from; `tests/entity_scenes.rs` re-derives every one of
/// them from the literal with the ported `Hash128` and checks the result against the shipped
/// files.
pub const WORLD_NODE_TYPE: [u8; 16] = id_bytes("E9FDFDEB4775443ABB236D4858232894");
/// `SerializeUtility.WorldFileType` @ 0x0 — the tag a scene *file* carries in `FileHeader.FileId`.
pub const WORLD_FILE_TYPE: [u8; 16] = id_bytes("7F090F7311DF4BA5BA3094AA33D0FAA7");
/// `SerializeUtility.DebugSectionNodeType` @ 0x20.
pub const DEBUG_SECTION_NODE_TYPE: [u8; 16] = id_bytes("582BAFE96EC44A72ACD9CCBC850FDBC4");
/// `SerializeUtility.TypesNameStringTableNodeType` @ 0x30.
pub const TYPES_NAME_STRING_TABLE_NODE_TYPE: [u8; 16] = id_bytes("4847374145504072849AAC7B1921E7FD");
/// `SerializeUtility.TypesNameNodeType` @ 0x40.
pub const TYPES_NAME_NODE_TYPE: [u8; 16] = id_bytes("828510FAD38F4EA78CD908D4D780388B");
/// `SerializeUtility.ArchetypesNodeType` @ 0x50.
pub const ARCHETYPES_NODE_TYPE: [u8; 16] = id_bytes("F5364E1CCB62466A9883F6F9554D4F0C");
/// `SerializeUtility.SharedAndManagedComponentsNodeType` @ 0x60.
pub const SHARED_AND_MANAGED_COMPONENTS_NODE_TYPE: [u8; 16] =
    id_bytes("D565355C5CF34C0DBBD4A06ADDA948B1");
/// `SerializeUtility.EnabledBitsNodeType` @ 0x70.
pub const ENABLED_BITS_NODE_TYPE: [u8; 16] = id_bytes("5846E500EA614C1DA94AFB85AFD8F4F4");
/// `SerializeUtility.BlobAssetsNodeType` @ 0x80.
pub const BLOB_ASSETS_NODE_TYPE: [u8; 16] = id_bytes("9A26954FF1ED4CC5A64E8AAD4F64773A");
/// `SerializeUtility.ChunksNodeType` @ 0x90.
pub const CHUNKS_NODE_TYPE: [u8; 16] = id_bytes("2EA7CE3325F04D7A84CEAB46790C628A");
/// `SerializeUtility.BufferDataNodeType` @ 0xA0.
pub const BUFFER_DATA_NODE_TYPE: [u8; 16] = id_bytes("E33124BFAC2649D792DE36E4611DDD70");
/// `SerializeUtility.PrefabNodeType` @ 0xB0.
pub const PREFAB_NODE_TYPE: [u8; 16] = id_bytes("2A84A183583A4FAD8CB7105AE3C47598");

/// The 16 bytes `Hash128.StringToHash(guid, guidFormatted: true)` produces for a literal: the
/// literal's bytes with the two nibbles of each byte swapped.
pub const fn id_bytes(guid: &str) -> [u8; 16] {
    let b = guid.as_bytes();
    let mut out = [0u8; 16];
    let mut i = 0;
    while i < 16 {
        out[i] = (nibble(b[2 * i + 1]) << 4) | nibble(b[2 * i]);
        i += 1;
    }
    out
}

const fn nibble(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

/// Which of `SerializeUtility`'s streams a node holds, named by its `Id`.
///
/// The payload of each one is written by the `Write*` body named on the variant and read back by
/// the `Read*` body beside it, all in `Unity.Entities.dll.c`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneStream {
    /// `WriteArchetypesNode` @ 0x8fe9ba0 / `ReadArchetypes` @ 0x8fe0fa0.
    Archetypes,
    /// `WriteBlobAssetNode` @ 0x8fe7ad0 / `WriteBlobAssetBatch` @ 0x8fe8380.
    BlobAssets,
    /// `WriteSharedAndManagedComponents` @ 0x8fe6d80 / `ReadSharedComponents` @ 0x8fec610.
    SharedAndManagedComponents,
    /// `WriteEnabledBits` @ 0x8fe7230.
    EnabledBits,
    /// `WriteChunks` @ 0x8fe65e0.
    Chunks,
    /// `WriteBufferDataElements` @ 0x8fe6080.
    BufferData,
    /// `WritePrefabNode` @ 0x8fe7f40.
    Prefab,
    /// `WriteTypeNames` @ 0x8fe9240.
    TypeNames,
    /// The `TypesNameStringTableNodeType` stream `WriteTypeNames` @ 0x8fe9240 emits alongside the
    /// type names.
    TypeNamesStringTable,
    /// The `DebugSectionNodeType` stream, present only in editor-built scenes.
    DebugSection,
}

/// Name a stream from its tag, or `None` for a tag this build never writes.
pub const fn stream_of_id(id: &[u8; 16]) -> Option<SceneStream> {
    match id {
        &ARCHETYPES_NODE_TYPE => Some(SceneStream::Archetypes),
        &BLOB_ASSETS_NODE_TYPE => Some(SceneStream::BlobAssets),
        &SHARED_AND_MANAGED_COMPONENTS_NODE_TYPE => Some(SceneStream::SharedAndManagedComponents),
        &ENABLED_BITS_NODE_TYPE => Some(SceneStream::EnabledBits),
        &CHUNKS_NODE_TYPE => Some(SceneStream::Chunks),
        &BUFFER_DATA_NODE_TYPE => Some(SceneStream::BufferData),
        &PREFAB_NODE_TYPE => Some(SceneStream::Prefab),
        &TYPES_NAME_NODE_TYPE => Some(SceneStream::TypeNames),
        &TYPES_NAME_STRING_TABLE_NODE_TYPE => Some(SceneStream::TypeNamesStringTable),
        &DEBUG_SECTION_NODE_TYPE => Some(SceneStream::DebugSection),
        _ => None,
    }
}

/// `DotsSerialization.FileHeader` — the 152-byte header of a scene file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHeader {
    /// `FileHeader.MagicValue` at 0x0.
    pub magic: u64,
    /// `FileHeader.FileVersion` at 0x8.
    pub file_version: i32,
    /// `FileHeader.HeaderSize` at 0xc.
    pub header_size: i32,
    /// `FileHeader.FileId` at 0x10, the raw 16 bytes of the `Hash128`.
    pub file_id: [u8; 16],
    /// `FileHeader.FileType` at 0x20, decoded from the `u16` length and the bytes that follow it.
    pub file_type: String,
    /// `FileHeader.FirstLevelNodesCount` at 0x60.
    pub first_level_nodes_count: i32,
    /// `FileHeader.NodesSectionOffset` at 0x68.
    pub nodes_section_offset: i64,
    /// `FileHeader.NodesSectionSize` at 0x70.
    pub nodes_section_size: i32,
    /// `FileHeader.MetadataSectionOffset` at 0x78.
    pub metadata_section_offset: i64,
    /// `FileHeader.MetadataSectionSize` at 0x80.
    pub metadata_section_size: i32,
    /// `FileHeader.DataSectionOffset` at 0x88.
    pub data_section_offset: i64,
    /// `FileHeader.DataSectionSize` at 0x90.
    pub data_section_size: i64,
}

fn i32_at(b: &[u8], at: usize) -> i32 {
    i32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn i64_at(b: &[u8], at: usize) -> i64 {
    i64::from_le_bytes([
        b[at],
        b[at + 1],
        b[at + 2],
        b[at + 3],
        b[at + 4],
        b[at + 5],
        b[at + 6],
        b[at + 7],
    ])
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes([
        b[at],
        b[at + 1],
        b[at + 2],
        b[at + 3],
        b[at + 4],
        b[at + 5],
        b[at + 6],
        b[at + 7],
    ])
}

impl FileHeader {
    /// Read the header of a `DOTSBIN!` file.
    pub fn read(b: &[u8]) -> Result<Self> {
        if b.len() < HEADER_SIZE {
            bail!("file is {} bytes, shorter than the {}-byte header", b.len(), HEADER_SIZE);
        }
        let magic = u64_at(b, 0x0);
        if magic != MAGIC {
            bail!("magic is {magic:#018x}, not DOTSBIN! ({MAGIC:#018x})");
        }
        // The dump types this field as a `FixedString64Bytes`; on disk it is a `u16` length at
        // 0x20 followed by that many bytes, inside a 64-byte field.
        let len = u16::from_le_bytes([b[0x20], b[0x21]]) as usize;
        if 0x22 + len > HEADER_SIZE {
            bail!("file type claims {len} bytes, which does not fit the header");
        }
        let mut out = String::new();
        for &c in &b[0x22..0x22 + len] {
            out.push(if c.is_ascii_graphic() || c == b' ' { c as char } else { '\u{fffd}' });
        }
        Ok(Self {
            magic,
            file_version: i32_at(b, 0x8),
            header_size: i32_at(b, 0xc),
            file_id: b[0x10..0x20].try_into().expect("16 bytes sliced"),
            file_type: out,
            first_level_nodes_count: i32_at(b, 0x60),
            nodes_section_offset: i64_at(b, 0x68),
            nodes_section_size: i32_at(b, 0x70),
            metadata_section_offset: i64_at(b, 0x78),
            metadata_section_size: i32_at(b, 0x80),
            data_section_offset: i64_at(b, 0x88),
            data_section_size: i64_at(b, 0x90),
        })
    }

    /// The three section sizes plus the header: the whole file, if the sections are contiguous
    /// and in the order the shipped files use (data, metadata, nodes).
    pub fn total_size(&self) -> i64 {
        self.header_size as i64
            + self.data_section_size
            + self.metadata_section_size as i64
            + self.nodes_section_size as i64
    }
}

/// `DotsSerialization.NodeHeader` — the 72-byte head of one node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeHeader {
    /// `NodeHeader.NodeTypeHash` at 0x0.
    pub node_type_hash: u64,
    /// `NodeHeader.Id` at 0x8, the raw 16 bytes of the `Hash128`.
    pub id: [u8; 16],
    /// `NodeHeader.Size` at 0x18 — the node's own byte size, header included.
    pub size: i32,
    /// `NodeHeader.NextSiblingOffset` at 0x1c — the next sibling, counted from the start of the
    /// nodes section, or `-1` when this is the last one.
    pub next_sibling_offset: i32,
    /// `NodeHeader.ChildrenCount` at 0x20.
    pub children_count: i32,
    /// `NodeHeader.MetadataStartingOffset` at 0x28.
    pub metadata_starting_offset: i64,
    /// `NodeHeader.MetadataSize` at 0x30.
    pub metadata_size: i32,
    /// `NodeHeader.DataStartingOffset` at 0x38 — an offset into the file, not into the section.
    pub data_starting_offset: i64,
    /// `NodeHeader.DataSize` at 0x40.
    pub data_size: i64,
}

impl NodeHeader {
    /// Read one node header.
    pub fn read(b: &[u8], at: usize) -> Result<Self> {
        if at + NODE_HEADER_SIZE > b.len() {
            bail!("node header at {at} runs past the end of {} bytes", b.len());
        }
        Ok(Self {
            node_type_hash: u64_at(b, at),
            id: b[at + 0x8..at + 0x18].try_into().expect("16 bytes sliced"),
            size: i32_at(b, at + 0x18),
            next_sibling_offset: i32_at(b, at + 0x1c),
            children_count: i32_at(b, at + 0x20),
            metadata_starting_offset: i64_at(b, at + 0x28),
            metadata_size: i32_at(b, at + 0x30),
            data_starting_offset: i64_at(b, at + 0x38),
            data_size: i64_at(b, at + 0x40),
        })
    }

    /// `NodeHeader.HasMetadata` — src: `DotsSerialization_NodeHeader__get_HasMetadata_0`
    /// @ 0x8fd7f70.
    pub fn has_metadata(&self) -> bool {
        self.metadata_starting_offset != -1
    }
}

/// The extra bytes a node type adds after the 72-byte header, keyed by what the dump lists.
///
/// The dump gives exactly four node types with a body of their own. Three of them are `NodeHeader`
/// plus one `int` at 0x48 and are indistinguishable by size, but the shipped scenes resolve the
/// ambiguity from the other side: `SerializeUtility..cctor` @ 0x8fef290 tags each stream with a
/// `Hash128`, every child node in every scene file carries one of those tags in its `Id`, and the
/// writer for that tag is `RevisionedRawDataNode`. So the port names the node from the tag and
/// keeps the tag's stream beside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// `DotsSerialization.FolderNode` — the header and nothing else.
    Folder,
    /// `DotsSerialization.StringTableNode` — `int StringCount; // 0x48`.
    StringTable {
        /// `StringTableNode.StringCount`.
        string_count: i32,
    },
    /// `DotsSerialization.TypeNamesNode` — `int TypeCount; // 0x48`.
    TypeNames {
        /// `TypeNamesNode.TypeCount`.
        type_count: i32,
    },
    /// `DotsSerialization.RevisionedRawDataNode` — `int Revision; // 0x48`, holding one of
    /// `SerializeUtility`'s streams.
    RevisionedRawData {
        /// `RevisionedRawDataNode.Revision`.
        revision: i32,
        /// Which stream `NodeHeader.Id` names. `None` for a tag this build never writes.
        stream: Option<SceneStream>,
    },
    /// A node whose type this port has not named yet: the hash and the trailing field are kept.
    Unknown {
        /// `NodeHeader.NodeTypeHash`.
        hash: u64,
        /// The `int` at 0x48, when the node is 80 bytes wide.
        extra: Option<i32>,
    },
}

/// One node of the tree, with the bytes its header points at.
#[derive(Clone, Debug)]
pub struct SceneNode {
    /// Where the node starts in the file.
    pub offset: usize,
    /// `NodeHeader`.
    pub header: NodeHeader,
    /// The node type, as far as the file lets this port name it.
    pub kind: NodeKind,
    /// Indices into `SceneFile::nodes` of this node's children, in file order.
    pub children: Vec<usize>,
    /// The bytes `DataStartingOffset .. + DataSize`, empty when the node has no data.
    pub data: Vec<u8>,
}

/// A parsed `DOTSBIN!` scene file: its header and its node tree.
pub struct SceneFile {
    /// `DotsSerialization.FileHeader`.
    pub header: FileHeader,
    /// Every node, in the order the tree walk reached them.
    pub nodes: Vec<SceneNode>,
    /// Indices into `nodes` of the `FirstLevelNodesCount` top-level nodes.
    pub roots: Vec<usize>,
    bytes: Vec<u8>,
}

impl SceneFile {
    /// Parse a scene file that is already in memory.
    pub fn parse(bytes: Vec<u8>) -> Result<Self> {
        let header = FileHeader::read(&bytes)
            .context("reading the DotsSerialization file header")?;
        if header.file_version != FILE_VERSION {
            bail!(
                "scene file version {} is not the {} this reader implements",
                header.file_version,
                FILE_VERSION
            );
        }
        let nodes_start = header.nodes_section_offset as usize;
        if nodes_start >= bytes.len() {
            bail!("nodes section offset {nodes_start} is past the end of the file");
        }
        let mut file = Self { header, nodes: Vec::new(), roots: Vec::new(), bytes };
        // The first-level nodes are the first `FirstLevelNodesCount` siblings of the chain that
        // starts at the head of the nodes section.
        let mut at = nodes_start;
        for _ in 0..file.header.first_level_nodes_count.max(0) {
            let idx = file.nodes.len();
            file.walk_node(at, nodes_start)?;
            file.roots.push(idx);
            let next = file.nodes[idx].header.next_sibling_offset;
            if next == -1 {
                break;
            }
            at = nodes_start + next as usize;
        }
        Ok(file)
    }

    /// Read and parse one `.entities` file.
    pub fn open(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading {}", path.display()))?;
        Self::parse(bytes).with_context(|| format!("parsing {}", path.display()))
    }

    fn walk_node(&mut self, at: usize, nodes_start: usize) -> Result<usize> {
        let header = NodeHeader::read(&self.bytes, at)?;
        if header.size < NODE_HEADER_SIZE as i32 {
            bail!("node at {at} claims a size of {}, below the {}-byte header",
                header.size, NODE_HEADER_SIZE);
        }
        let end = at + header.size as usize;
        if end > self.bytes.len() {
            bail!("node at {at} runs to {end}, past the end of {} bytes", self.bytes.len());
        }
        let kind = classify(&header, &self.bytes[at + NODE_HEADER_SIZE..end]);
        let data = if header.data_size > 0 {
            let s = header.data_starting_offset as usize;
            let e = s + header.data_size as usize;
            if e > self.bytes.len() {
                bail!("node at {at} points at data {s}..{e}, past the end of {} bytes",
                    self.bytes.len());
            }
            self.bytes[s..e].to_vec()
        } else {
            Vec::new()
        };
        let children_start = at + header.size as usize;
        let index = self.nodes.len();
        self.nodes.push(SceneNode { offset: at, header, kind, children: Vec::new(), data });
        if header.children_count > 0 {
            let mut child = children_start;
            for _ in 0..header.children_count {
                let idx = self.walk_node(child, nodes_start)?;
                self.nodes[index].children.push(idx);
                let next = self.nodes[idx].header.next_sibling_offset;
                if next == -1 {
                    break;
                }
                child = nodes_start + next as usize;
            }
        }
        Ok(index)
    }

    /// The raw bytes of the file, still available for the decoders that need the data section.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Every node that carries a data blob, in file order.
    pub fn blobs(&self) -> impl Iterator<Item = &SceneNode> {
        self.nodes.iter().filter(|n| n.header.data_size > 0)
    }

    /// The data blob of one `SerializeUtility` stream, if this file carries it.
    pub fn stream(&self, stream: SceneStream) -> Option<&SceneNode> {
        self.blobs().find(|n| matches!(n.kind, NodeKind::RevisionedRawData { stream: s, .. } if s == Some(stream)))
    }

    /// The `Chunks` stream as the chunk blocks it actually is.
    ///
    /// src: `SerializeUtility.WriteChunks` @ 0x8fe65e0. For every chunk of every archetype the body
    /// does `UnsafeUtility.MemCpy(tempChunk, ChunkIndex.GetPtr(chunkIndex), 0x8000)` — a whole
    /// 32768-byte chunk allocation — then patches the two fields the file has to carry that the
    /// live chunk keeps elsewhere (`CountForSerialization` at 0x10, and `metaChunkEntity` at 0x8
    /// through `EntityRemapUtility.RemapEntity`), patches the buffer headers with
    /// `BufferHeader.PatchAfterCloningChunk`, and writes the block out as `0x8000` bytes. Nothing
    /// else goes into this stream: no count, no per-chunk framing, so it is exactly `chunkCount`
    /// blocks of `Chunk.kChunkSize`.
    ///
    /// Measured on the install: 1183 files, 31723 blocks, every size a multiple of 32768, every
    /// `CountForSerialization` in `0..=4088`, every `Flags` one of the four `ChunkFlags` values,
    /// 1385671 entities described in total. See `evidence/scene_chunks.txt`.
    pub fn chunks(&self) -> Result<Vec<SceneChunk<'_>>> {
        let Some(node) = self.stream(SceneStream::Chunks) else { return Ok(Vec::new()) };
        let base = node.header.data_starting_offset as usize;
        let len = node.header.data_size as usize;
        if base + len > self.bytes.len() {
            bail!("chunks stream at {base} claims {len} bytes, past the end of the file");
        }
        if len % rrising_ecs::K_CHUNK_SIZE != 0 {
            bail!(
                "chunks stream is {len} bytes, not a multiple of the {}-byte chunk size",
                rrising_ecs::K_CHUNK_SIZE
            );
        }
        let blocks = self.bytes[base..base + len].chunks_exact(rrising_ecs::K_CHUNK_SIZE);
        let mut out = Vec::with_capacity(blocks.len());
        for (i, block) in blocks.enumerate() {
            let Some(header) = rrising_ecs::Chunk::read_serialized_header(block) else {
                bail!("chunk block {i} is too short for a serialized chunk header");
            };
            if header.count < 0 || header.count > rrising_ecs::K_MAXIMUM_ENTITIES_PER_CHUNK {
                bail!(
                    "chunk block {i} claims {} entities, outside 0..={}",
                    header.count,
                    rrising_ecs::K_MAXIMUM_ENTITIES_PER_CHUNK
                );
            }
            out.push(SceneChunk { index: i, header, buffer: &block[rrising_ecs::K_BUFFER_OFFSET..] });
        }
        Ok(out)
    }

    /// The `Archetypes` stream: the archetype list plus the stable type hashes it indexes.
    ///
    /// src: `SerializeUtility.WriteArchetypesNode` @ 0x8fe9ba0 writes, into the node's data blob,
    /// `int UniqueTypesCount`, then `UniqueTypesCount × ulong` of `TypeInfo.Hash` (`+0x18`, the
    /// stable type hash), then `int ArchetypeCount`, then per archetype
    /// `int Archetype.EntityCount` (`+0xbc`), `int Archetype.TypesCount - 1` (`+0xc4`) — the
    /// `-1` skips the `SharedComponentData` entry every live archetype keeps at `Types[0]` — and
    /// that many `int`s of `uniqueIndex | 0x20000000` for a chunk component.
    /// `SerializeUtility.ReadArchetypes` @ 0x8fe0fa0 reads the same shape back: it maps each
    /// stored int through the type table as `types[raw & 0x3fff]`, ORs `0x60000000`
    /// (`ChunkComponentTypeFlag | ZeroSizeInChunkTypeFlag`) in when the `0x20000000` bit is set, and
    /// hands the result to
    /// `EntityComponentStore.CreateArchetypeWithoutSimulateComponent`.
    ///
    /// Measured on the install: 1183 files, 17579 archetypes, 32974 unique type entries, every
    /// stored int carrying only bits 0..13 and `0x20000000`, every reference in range, and in
    /// every one of the 1183 files the stream's `EntityCount` sum equals the
    /// `CountForSerialization` sum of that file's own `Chunks` stream — 1385671 entities
    /// altogether. See `evidence/entity_scenes_fingerprint.txt`.
    pub fn archetypes(&self) -> Result<SceneArchetypes> {
        let Some(node) = self.stream(SceneStream::Archetypes) else {
            return Ok(SceneArchetypes::default());
        };
        let mut c = Cursor::new(&node.data, "archetypes stream");
        let unique_count = c.i32()?;
        if unique_count < 0 {
            bail!("archetypes stream claims {unique_count} unique types");
        }
        let mut unique_type_hashes = Vec::with_capacity(unique_count as usize);
        for _ in 0..unique_count {
            unique_type_hashes.push(c.u64()?);
        }
        let archetype_count = c.i32()?;
        if archetype_count < 0 {
            bail!("archetypes stream claims {archetype_count} archetypes");
        }
        let mut archetypes = Vec::with_capacity(archetype_count as usize);
        for i in 0..archetype_count {
            let entity_count = c.i32()?;
            let type_count = c.i32()?;
            if type_count < 0 {
                bail!("archetype {i} claims {type_count} component types");
            }
            let mut types = Vec::with_capacity(type_count as usize);
            for _ in 0..type_count {
                let raw = c.u32()?;
                if raw & !(ARCHETYPE_TYPE_INDEX_MASK | ARCHETYPE_TYPE_CHUNK_COMPONENT) != 0 {
                    bail!(
                        "archetype {i} stores component value {raw:#010x}, with bits outside the \
                         14-bit index and the chunk-component flag"
                    );
                }
                let unique_index = (raw & ARCHETYPE_TYPE_INDEX_MASK) as u16;
                if usize::from(unique_index) >= unique_count as usize {
                    bail!(
                        "archetype {i} references unique type {unique_index}, of which the stream \
                         lists {unique_count}"
                    );
                }
                types.push(SerializedArchetypeType {
                    unique_index,
                    chunk_component: raw & ARCHETYPE_TYPE_CHUNK_COMPONENT != 0,
                });
            }
            archetypes.push(SceneArchetype { entity_count, types });
        }
        if !c.at_end() {
            bail!("archetypes stream leaves {} bytes after the last archetype", c.remaining());
        }
        Ok(SceneArchetypes { unique_type_hashes, archetypes })
    }

    /// The `BufferData` stream: the `DynamicBuffer` contents the chunk blocks leave out.
    ///
    /// src: `SerializeUtility.WriteBufferDataElements` @ 0x8fe6080. Its signature
    /// (`dump.cs`, `Unity.Entities.Serialization.SerializeUtility`) is
    /// `WriteBufferDataElements(DotsSerializationWriter, int totalChunkCount,
    /// UnsafePtrList<Archetype> archetypeArray, NativeList<int> bufferPatchesCountPerChunk,
    /// NativeList<BufferPatchRecord> bufferPatches, NativeList<IntPtr> bufferDataList)`, and the
    /// body writes `int totalChunkCount`, then for every archetype in `archetypeArray` — the list
    /// `WriteArchetypesNode` returned, so in `Archetypes`-stream order — one `int` patch count per
    /// chunk of that archetype (`Archetype.Chunks.Count`, the `ArchetypeChunkData.Count` field at
    /// `+0xc`), and for every patch the 8-byte `BufferPatchRecord` (`ChunkOffset` @ 0x0,
    /// `AllocSizeBytes` @ 0x4) followed straight away by `AllocSizeBytes` bytes taken from the
    /// matching `bufferDataList` entry.
    ///
    /// Those lists were filled by `SerializeUtility.FillBufferPatchRecordsAndClearBufferPointer`
    /// @ 0x8fee550: for every entity of every buffer component of the archetype it appends
    /// `ChunkOffset` as the component's byte offset inside the chunk buffer, `AllocSizeBytes` as
    /// the element size times the buffer's element count, appends the live buffer pointer, and
    /// zeroes the pointer in the chunk — which is why the chunk blocks hold nothing at these
    /// offsets and why this stream has to exist at all.
    ///
    /// Measured on the install: 1183 files, 31723 chunk groups, 89120 patches, 339707598 bytes of
    /// buffer contents, every stream consumed exactly to its `DataSize`, every group count summing
    /// to its file's chunk blocks, and all 89120 patches landing on eight zero bytes at
    /// `block + Chunk.kBufferOffset + ChunkOffset` with strictly increasing offsets per chunk. See
    /// `evidence/entity_buffer_data.txt`.
    pub fn buffer_data(&self) -> Result<SceneBufferData<'_>> {
        let Some(node) = self.stream(SceneStream::BufferData) else {
            return Ok(SceneBufferData::default());
        };
        let chunks = self.chunks()?;
        let mut c = Cursor::new(&node.data, "buffer data stream");
        let total_chunk_count = c.i32()?;
        if total_chunk_count < 0 || total_chunk_count as usize != chunks.len() {
            bail!(
                "buffer data stream claims {total_chunk_count} chunks, the chunks stream holds {}",
                chunks.len()
            );
        }
        let mut per_chunk = Vec::with_capacity(chunks.len());
        for chunk in &chunks {
            let patch_count = c.i32()?;
            if patch_count < 0 {
                bail!("chunk {} claims {patch_count} buffer patches", chunk.index);
            }
            let mut patches = Vec::with_capacity(patch_count as usize);
            for j in 0..patch_count {
                let chunk_offset = c.i32()?;
                let alloc_size_bytes = c.i32()?;
                if chunk_offset < 0
                    || chunk_offset as usize + 8 > rrising_ecs::K_BUFFER_SIZE
                {
                    bail!(
                        "chunk {} patch {j} points at buffer offset {chunk_offset}, outside the \
                         {} bytes a chunk buffer holds",
                        chunk.index,
                        rrising_ecs::K_BUFFER_SIZE
                    );
                }
                if alloc_size_bytes < 0 {
                    bail!(
                        "chunk {} patch {j} claims {alloc_size_bytes} bytes of buffer contents",
                        chunk.index
                    );
                }
                let len = alloc_size_bytes as usize;
                if c.remaining() < len {
                    bail!(
                        "chunk {} patch {j} claims {len} bytes of buffer contents, {} left in the \
                         stream",
                        chunk.index,
                        c.remaining()
                    );
                }
                let data = &node.data[c.at..c.at + len];
                c.at += len;
                patches.push(BufferPatch { chunk_offset, alloc_size_bytes, data });
            }
            per_chunk.push(ChunkBufferData { chunk_index: chunk.index, patches });
        }
        if !c.at_end() {
            bail!("buffer data stream leaves {} bytes after the last chunk", c.remaining());
        }
        Ok(SceneBufferData { total_chunk_count, per_chunk })
    }

    /// The `EnabledBits` stream: which entity each component is enabled for, chunk by chunk.
    ///
    /// src: `SerializeUtility.WriteEnabledBits` @ 0x8fe7230. Its signature is
    /// `WriteEnabledBits(DotsSerializationWriter, UnsafePtrList<Archetype> archetypeArray)`. The
    /// body first sizes two regions by walking the archetypes —
    /// `total += Archetype.Chunks.ComponentCount * Archetype.Chunks.Count * 16` and
    /// `total += ... * 4`, where `ArchetypeChunkData` keeps `Count` at `+0xc`, `Capacity` at `+0x8`,
    /// `SharedComponentCount` at `+0x10` and `ComponentCount` at `+0x14` — then writes
    /// `int totalEnabledBitsSize`, one archetype's worth of `v128`s after another,
    /// `int totalHierarchicalDataSize`, and one archetype's worth of `int`s after another. Each
    /// 16-byte unit is `ArchetypeChunkData.ComponentEnabledBits` for one (chunk, component) pair,
    /// which is what `GetComponentEnabledMaskArrayForChunk` @ 0x911D5E0 hands out as
    /// `chunkIndex * ComponentEnabledBitsSizeTotalPerChunk`, and each `int` is the matching entry
    /// of `ComponentEnabledBitsHierarchicalData` — how many entities of that chunk the component is
    /// *disabled* for. `GetChunkDisabledCountForType` @ 0x911DB10 reads one back, and
    /// `SerializeUtilityInterop.AddExistingChunk` @ 0x8FDEBC0 hands the region to
    /// `SetEnabledBitsAndHierarchicalData` @ 0x911D920 as `perComponentDisabledBitCount` when the
    /// scene is imported. `Archetype.Chunks` is an inline `ArchetypeChunkData` at `+0x0`, which is
    /// why the writer reads the counts at `+0xc` and `+0x14`.
    ///
    /// `ComponentCount` is the archetype's full `TypesCount`, one more than the component slots the
    /// `Archetypes` stream lists, because that stream leaves out the `SharedComponentData` entry
    /// `CreateArchetype` puts at `Types[0]` while the chunk data still counts it.
    ///
    /// Measured on the install: 1183 files, 648652 (chunk, component) units, 10378432 bytes of
    /// masks and 2594608 bytes of hierarchical data, every stream exactly `8 + 20 × units` bytes
    /// long. No mask sets a bit at or above its chunk's `CountForSerialization` and no disabled
    /// count exceeds it. Of the 621658 units that are not chunk components, 45360 have every entity
    /// bit set with a zero count, 162 have a zero mask with a count equal to the chunk's entity
    /// count — a component disabled for the whole chunk — and the rest are zero in both, because
    /// the store keeps these arrays live only for enableable types. No unit is partially set. All
    /// 26994 chunk-component units are zero in both arrays. See `evidence/entity_enabled_bits.txt`.
    pub fn enabled_bits(&self) -> Result<SceneEnabledBits> {
        let Some(node) = self.stream(SceneStream::EnabledBits) else {
            return Ok(SceneEnabledBits::default());
        };
        let arch = self.archetypes()?;
        let chunks = self.chunks()?;
        let mut chunk_counts = vec![0usize; arch.archetypes.len()];
        for c in &chunks {
            let i = usize::try_from(c.header.archetype_index).map_err(|_| {
                anyhow::anyhow!("chunk {} names archetype {}", c.index, c.header.archetype_index)
            })?;
            let Some(slot) = chunk_counts.get_mut(i) else {
                bail!(
                    "chunk {} names archetype {i}, of which the archetypes stream lists {}",
                    c.index,
                    chunk_counts.len()
                );
            };
            *slot += 1;
        }
        let component_counts: Vec<usize> =
            arch.archetypes.iter().map(|a| a.types.len() + 1).collect();
        let mut units: usize = 0;
        for (c, m) in chunk_counts.iter().zip(component_counts.iter()) {
            units += c * m;
        }
        let mut c = Cursor::new(&node.data, "enabled bits stream");
        let total_enabled_bits_size = c.i32()?;
        let mask_bytes = units.checked_mul(16).ok_or_else(|| anyhow::anyhow!("{units} component-chunk units overflow"))?;
        if total_enabled_bits_size < 0 || total_enabled_bits_size as usize != mask_bytes {
            bail!(
                "enabled bits stream claims {total_enabled_bits_size} bytes of masks, the {} chunk \
                 blocks over {} component slots make {}",
                chunks.len(),
                units,
                mask_bytes
            );
        }
        let mut masks = Vec::with_capacity(units);
        for _ in 0..units {
            masks.push(ChunkEnabledMask { low: c.u64()?, high: c.u64()? });
        }
        let total_hierarchical_size = c.i32()?;
        let hier_bytes = units.checked_mul(4).ok_or_else(|| anyhow::anyhow!("{units} component-chunk units overflow"))?;
        if total_hierarchical_size < 0 || total_hierarchical_size as usize != hier_bytes {
            bail!(
                "enabled bits stream claims {total_hierarchical_size} bytes of hierarchical data, \
                 {} component-chunk units make {}",
                units,
                hier_bytes
            );
        }
        let mut hierarchical = Vec::with_capacity(units);
        for _ in 0..units {
            hierarchical.push(c.i32()?);
        }
        if !c.at_end() {
            bail!("enabled bits stream leaves {} bytes after the last unit", c.remaining());
        }
        Ok(SceneEnabledBits { chunk_counts, component_counts, masks, hierarchical })
    }

    /// The `SharedAndManagedComponents` stream: the shared component values and the managed
    /// objects that the chunk blocks reference only by index.
    ///
    /// src: `SerializeUtility.WriteSharedAndManagedComponents` @ 0x8fe6d80 writes
    /// `int sharedComponentArrays.Length` and then `Length * 4` raw bytes of it
    /// (`BinaryWriterExtensions.Write` @ 0x8fd6c30, then the raw `WriteBytes` tail
    /// `FUN_180034510`), then calls `WriteUnmanagedSharedComponents` @ 0x8feb380 and the managed
    /// half @ 0x8fea7c0. `EndDeserializeWorld` @ 0x8fe34f0 reads the same two fields first, into
    /// the `NativeList<int> sharedComponentRemap` it then hands to both readers.
    ///
    /// `WriteUnmanagedSharedComponents` @ 0x8feb380 calls
    /// `WriteUnmanagedSharedComponent` @ 0x8feb900 once per unmanaged shared component index; that
    /// returns one 16-byte `SerializeUtility.SharedComponentRecord` — `ulong StableTypeHash` @ 0x0,
    /// `int ComponentSize` @ 0x8 — and appends the value's bytes to an `UnsafeAppendBuffer`. It then
    /// writes `int count`, `count * 16` record bytes, `int buffer.Length` and the buffer.
    /// `ReadUnmanagedSharedComponents` @ 0x8febba0 reads it back and walks the buffer with a running
    /// offset, one `ComponentSize` at a time, into
    /// `EntityComponentStore.InsertSharedComponent_Unmanaged`.
    ///
    /// The managed half @ 0x8fea7c0 writes every managed shared component object with
    /// `ManagedObjectBinaryWriter.WriteObject` into one `UnsafeAppendBuffer` and records
    /// `{ StableTypeHash, ComponentSize = 0xffffffff }` for it — the writer stores the two dwords
    /// `0xffffffff, 0`, so `ComponentSize` reads back as `-1` and the four bytes after it are zero.
    /// It then walks every archetype's managed components (`FirstManagedComponent` @ 0x11a to
    /// `FirstSharedComponent` @ 0x11c) and appends `ulong TypeInfo.Hash` (`+0x18`) plus the object
    /// for each, counting them. The stream gets `int count`, `count * 16` record bytes,
    /// `int buffer.Length`, `int managedComponentCount` and the buffer.
    /// `ReadManagedSharedComponents` @ 0x8febfd0 reads the blob into a `ManagedObjectBinaryReader`,
    /// has `ReadSharedComponents` @ 0x8fec610 pull one object per record off the front, then loops
    /// `managedComponentCount` times reading a hash and one object.
    ///
    /// Measured on the install: 1183 files carry the stream and every one is consumed exactly to its
    /// `DataSize`, 9414749 bytes in all — 82485 remap entries, 3366 unmanaged shared components over
    /// 67054 value bytes, 903 managed shared components, and 8921059 bytes of managed blob holding
    /// 57578 managed components. The unmanaged sizes always sum to the declared total, every managed
    /// record's `ComponentSize` is `-1` with zero padding, and in every file the non-zero remap
    /// values are exactly `1..=unmanaged + managed`, so the table maps onto the serialized list. All
    /// 4269 record hashes, and the leading hash of the first managed component in the 771 files whose
    /// blob starts at one, appear in that same file's `Archetypes` unique type list. See
    /// `evidence/entity_shared_and_managed_components.txt`.
    pub fn shared_and_managed_components(&self) -> Result<SceneSharedComponents<'_>> {
        let Some(node) = self.stream(SceneStream::SharedAndManagedComponents) else {
            return Ok(SceneSharedComponents::default());
        };
        let mut c = Cursor::new(&node.data, "shared and managed components stream");
        let remap_count = c.i32()?;
        if remap_count < 0 {
            bail!(
                "shared and managed components stream claims {remap_count} shared component indices"
            );
        }
        let mut shared_component_remap = Vec::with_capacity(remap_count as usize);
        for _ in 0..remap_count {
            shared_component_remap.push(c.i32()?);
        }

        let unmanaged_count = c.i32()?;
        if unmanaged_count < 0 {
            bail!("stream claims {unmanaged_count} unmanaged shared components");
        }
        let mut unmanaged_meta = Vec::with_capacity(unmanaged_count as usize);
        for i in 0..unmanaged_count {
            let record = read_shared_component_record(&mut c, i)?;
            if record.component_size < 0 {
                bail!(
                    "unmanaged shared component {i} declares a component size of {}, which is not a \
                     number of bytes",
                    record.component_size
                );
            }
            unmanaged_meta.push(record);
        }
        let unmanaged_total = c.i32()?;
        if unmanaged_total < 0 {
            bail!("stream claims {unmanaged_total} bytes of unmanaged shared component values");
        }
        let summed: i64 = unmanaged_meta.iter().map(|r| i64::from(r.component_size)).sum();
        if summed != i64::from(unmanaged_total) {
            bail!(
                "the {unmanaged_count} unmanaged shared component records total {summed} bytes, the \
                 stream declares {unmanaged_total}"
            );
        }
        let mut unmanaged = Vec::with_capacity(unmanaged_count as usize);
        for record in unmanaged_meta {
            let data = c.take(record.component_size as usize)?;
            unmanaged.push(UnmanagedSharedComponent {
                stable_type_hash: record.stable_type_hash,
                component_size: record.component_size,
                data,
            });
        }

        let managed_shared_count = c.i32()?;
        if managed_shared_count < 0 {
            bail!("stream claims {managed_shared_count} managed shared components");
        }
        let mut managed_shared_records = Vec::with_capacity(managed_shared_count as usize);
        for i in 0..managed_shared_count {
            let record = read_shared_component_record(&mut c, i)?;
            if record.component_size != SHARED_COMPONENT_MANAGED_SIZE {
                bail!(
                    "managed shared component {i} declares a component size of {}, the writer stores \
                     {SHARED_COMPONENT_MANAGED_SIZE} for every managed object",
                    record.component_size
                );
            }
            managed_shared_records.push(record);
        }
        let blob_size = c.i32()?;
        if blob_size < 0 {
            bail!("stream claims a managed blob of {blob_size} bytes");
        }
        let managed_component_count = c.i32()?;
        if managed_component_count < 0 {
            bail!("stream claims {managed_component_count} managed components");
        }
        let managed_blob = c.take(blob_size as usize)?;
        if !c.at_end() {
            bail!(
                "shared and managed components stream leaves {} bytes after the managed blob",
                c.remaining()
            );
        }

        let serialized = unmanaged.len() + managed_shared_records.len();
        for (i, &value) in shared_component_remap.iter().enumerate() {
            if value == 0 {
                continue;
            }
            let Ok(index) = usize::try_from(value - 1) else {
                bail!("shared component remap entry {i} is {value}, not a serialized index");
            };
            if index >= serialized {
                bail!(
                    "shared component remap entry {i} points at serialized shared component {index}, \
                     of which this file has {serialized}"
                );
            }
        }
        Ok(SceneSharedComponents {
            shared_component_remap,
            unmanaged,
            managed_shared_records,
            managed_blob,
            managed_component_count,
        })
    }

    /// Decode the `BlobAssets` stream: one `Unity.Entities.BlobAssetBatch`.
    ///
    /// src: `SerializeUtility.WriteBlobAssetNode` @ 0x8fe7ad0 opens a `RevisionedRawDataNode`
    /// tagged `BlobAssetsNodeType` @ 0x80, collects every blob asset the world reaches with
    /// `GatherAllUsedBlobAssets` @ 0x8feceb0, sizes the batch with
    /// `CalculateBlobAssetBatchTotalSize` @ 0x8fe81f0 and writes it with `WriteBlobAssetBatch`
    /// @ 0x8fe8380. The batch front is `BlobAssetBatch.CreateForSerialize` @ 0x9070490 —
    /// `{ int TotalDataSize; int BlobAssetHeaderCount; int RefCount; int Padding }` with
    /// `RefCount = 1` and `Padding = 0` — and each blob is `BlobAssetHeader.CreateForSerialize`
    /// @ 0x90709c0's 32 bytes, `{ void* ValidationPtr; int Length; AllocatorHandle Allocator;
    /// ulong Hash; ulong Padding }` with `ValidationPtr = 0`, `Allocator = 1` and `Padding = 0`,
    /// followed by the blob's bytes and then zero padding up to a multiple of 16.
    /// `CalculateBlobAssetBatchTotalSize` sizes the batch as `16 + sum(32 + align16(Length))` and
    /// records each blob's payload at `48 + sum(32 + align16(Length))` over the earlier blobs,
    /// which is where the writer puts the bytes and what the offsets it hands back count from.
    ///
    /// `SerializeUtility.EndDeserializeWorld` @ 0x8fe34f0 hands the node's whole data blob to
    /// `BlobAssetBatch.CreateFromMemory` @ 0x90704b0, which rejects a batch whose `TotalDataSize`
    /// is not the buffer size or whose `RefCount` is not 1, then walks the headers exactly as here
    /// — patching each `ValidationPtr` to the bytes after its own header and rejecting any
    /// `Allocator` that is not 1 — so the batch is used in place, with no copy and no index built.
    ///
    /// Those payload offsets are what a serialized shared component stores in each of its
    /// `BlobAssetReferenceData` fields. `ReadUnmanagedSharedComponents` @ 0x8febba0 keeps
    /// `WorldDeserializationStatus.BlobAssetBuffer` @ 0x28 — the base of this very buffer — and
    /// for each of the `TypeInfo.BlobAssetRefCount` @ 0x3c reference slots listed by
    /// `TypeManager.GetBlobAssetRefOffsetsPointer` reads the `int` the stream left in the value
    /// and writes back `base + value`, or a null pointer when that `int` is `-1`.
    ///
    /// Measured on the install: 1183 files carry the stream and every one is a batch the runtime
    /// validator accepts — `TotalDataSize` equals the node's `DataSize`, `RefCount` is 1 in all
    /// 1183, and walking `32 + Length` from the front lands exactly on `DataSize` in all 1183.
    /// 762 files hold an empty batch of 16 bytes; the other 421 hold 85442 blobs over
    /// 899199248 bytes, every `Length` a multiple of 16 (so the padding step writes nothing),
    /// every `ValidationPtr` and `Padding` zero and every `Allocator` 1. 50768 distinct `Hash`
    /// values appear across the install, so the same blob is shared between scenes. See
    /// `evidence/entity_blob_assets.txt`.
    pub fn blob_assets(&self) -> Result<SceneBlobAssets<'_>> {
        let Some(node) = self.stream(SceneStream::BlobAssets) else {
            return Ok(SceneBlobAssets::default());
        };
        let mut c = Cursor::new(&node.data, "blob assets stream");
        let header = BlobAssetBatchHeader {
            total_data_size: c.i32()?,
            blob_asset_header_count: c.i32()?,
            ref_count: c.i32()?,
            padding: c.i32()?,
        };
        if i64::from(header.total_data_size) != node.data.len() as i64 {
            bail!(
                "the blob asset batch declares {} bytes, the node holds {} of them",
                header.total_data_size,
                node.data.len()
            );
        }
        if header.blob_asset_header_count < 0 {
            bail!(
                "the blob asset batch declares {} blob assets",
                header.blob_asset_header_count
            );
        }
        if header.ref_count != BLOB_ASSET_BATCH_REF_COUNT {
            bail!(
                "the blob asset batch carries a reference count of {}, \
                 `CreateForSerialize` stores {BLOB_ASSET_BATCH_REF_COUNT}",
                header.ref_count
            );
        }
        if header.padding != 0 {
            bail!(
                "the blob asset batch carries {:#010x} in its four padding bytes",
                header.padding
            );
        }

        let mut blobs = Vec::with_capacity(header.blob_asset_header_count as usize);
        for i in 0..header.blob_asset_header_count {
            let validation_ptr = c.u64()?;
            let length = c.i32()?;
            let allocator = c.u32()?;
            let hash = c.u64()?;
            let tail_padding = c.u64()?;
            if validation_ptr != 0 {
                bail!(
                    "blob asset {i} carries {validation_ptr:#018x} in its `ValidationPtr`, \
                     the writer stores zero"
                );
            }
            if length < 0 || length % BLOB_ASSET_ALIGNMENT != 0 {
                bail!(
                    "blob asset {i} declares a length of {length}, not a non-negative multiple \
                     of {BLOB_ASSET_ALIGNMENT}"
                );
            }
            if allocator != BLOB_ASSET_HEADER_ALLOCATOR {
                bail!(
                    "blob asset {i} carries allocator {allocator}, `CreateForSerialize` stores \
                     {BLOB_ASSET_HEADER_ALLOCATOR}"
                );
            }
            if tail_padding != 0 {
                bail!(
                    "blob asset {i} carries {tail_padding:#018x} in its eight padding bytes"
                );
            }
            let offset = c.at;
            let data = c.take(length as usize)?;
            blobs.push(SceneBlobAsset {
                index: i as usize,
                offset,
                hash,
                length,
                data,
            });
        }
        if !c.at_end() {
            bail!(
                "blob assets stream leaves {} bytes after the last blob",
                c.remaining()
            );
        }
        Ok(SceneBlobAssets { header, blobs })
    }
}

/// One block of the `Chunks` stream: a chunk's serialized header and its component bytes.
#[derive(Clone, Copy, Debug)]
pub struct SceneChunk<'a> {
    /// Position of this block in the stream, counting from zero.
    pub index: usize,
    /// The `Chunk` header fields, read from the front of the block.
    pub header: rrising_ecs::ChunkHeader,
    /// The bytes after `Chunk.kBufferOffset`.
    pub buffer: &'a [u8],
}

/// The bit `SerializeUtility.WriteArchetypes` @ 0x8feeed0 copies out of a component's live
/// `TypeIndex` value: `TypeManager.ChunkComponentTypeFlag` @ 0x20000000, set for the one component
/// type that lives in the chunk header rather than per entity.
///
/// src: `TypeManager` constants (`dump.cs`, TypeDefIndex 23422) — `ChunkComponentTypeFlag =
/// 536870912`, `ZeroSizeInChunkTypeFlag = 1073741824`.
pub const ARCHETYPE_TYPE_CHUNK_COMPONENT: u32 = 0x2000_0000;
/// The mask `SerializeUtility.ReadArchetypes` @ 0x8fe0fa0 applies with `raw & 0x3fff` — the
/// `TypeIndex.Index` half of a packed `TypeIndex`, which here indexes the stream's own table.
pub const ARCHETYPE_TYPE_INDEX_MASK: u32 = 0x3fff;

/// The `Archetypes` stream of one scene file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SceneArchetypes {
    /// The stable type hash of every component type the file uses, in stream order.
    ///
    /// `SerializeUtility.ReadTypeArray` @ 0x8fe13e0 turns this list into live `TypeIndex`es with
    /// `TypeManager.GetTypeIndexFromStableTypeHash` @ 0x8fd2e20, falling back to the name the
    /// `TypeNames` stream carries for any hash the running game does not know.
    pub unique_type_hashes: Vec<u64>,
    /// The archetypes, in stream order.
    pub archetypes: Vec<SceneArchetype>,
}

impl SceneArchetypes {
    /// The entities the stream describes, which is the value `ReadArchetypes` @ 0x8fe0fa0
    /// accumulates into its `out int` parameter as it walks the archetypes.
    pub fn total_entity_count(&self) -> i64 {
        self.archetypes.iter().map(|a| i64::from(a.entity_count)).sum()
    }
}

/// One archetype of the `Archetypes` stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneArchetype {
    /// `Archetype.EntityCount` @ 0xbc as the writer saw it.
    pub entity_count: i32,
    /// The component slots, `Archetype.TypesCount @ 0xc4` minus the `SharedComponentData` entry
    /// the live archetype keeps first and the stream leaves out.
    pub types: Vec<SerializedArchetypeType>,
}

/// One component slot as the `Archetypes` stream stores it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SerializedArchetypeType {
    /// Its index in `SceneArchetypes::unique_type_hashes`.
    pub unique_index: u16,
    /// Whether the stored int carried `ARCHETYPE_TYPE_CHUNK_COMPONENT`, so the component is a chunk
    /// component.
    pub chunk_component: bool,
}

impl SerializedArchetypeType {
    /// The int the stream stores for this slot, for writing a scene back out.
    pub fn to_raw(self) -> u32 {
        u32::from(self.unique_index)
            | if self.chunk_component { ARCHETYPE_TYPE_CHUNK_COMPONENT } else { 0 }
    }
}

/// The `BufferData` stream of one scene file.
#[derive(Clone, Debug, Default)]
pub struct SceneBufferData<'a> {
    /// `totalChunkCount` as `WriteBufferDataElements` @ 0x8fe6080 received it: the number of
    /// chunks in the file's `Chunks` stream, which is also the number of patch groups here.
    pub total_chunk_count: i32,
    /// One group per chunk block, in stream order — the writer walks `archetypeArray` and, inside
    /// each archetype, that archetype's chunks, which is the same order `WriteChunks` @ 0x8fe65e0
    /// writes the blocks in.
    pub per_chunk: Vec<ChunkBufferData<'a>>,
}

impl SceneBufferData<'_> {
    /// The patches across all chunks.
    pub fn patch_count(&self) -> usize {
        self.per_chunk.iter().map(|c| c.patches.len()).sum()
    }

    /// The bytes of `DynamicBuffer` contents across all patches.
    pub fn total_data_bytes(&self) -> usize {
        self.per_chunk.iter().flat_map(|c| c.patches.iter()).map(|p| p.data.len()).sum()
    }
}

/// The `BufferData` patches belonging to one chunk block.
#[derive(Clone, Debug)]
pub struct ChunkBufferData<'a> {
    /// The block these patches patch, as indexed by `SceneFile::chunks`.
    pub chunk_index: usize,
    /// The patches, in stream order. `FillBufferPatchRecordsAndClearBufferPointer` @ 0x8fee550
    /// walks entities and then the archetype's buffer components, so these offsets ascend.
    pub patches: Vec<BufferPatch<'a>>,
}

/// One `SerializeUtility.BufferPatchRecord` and the buffer contents written right after it.
///
/// src: `internal struct SerializeUtility.BufferPatchRecord { public int ChunkOffset; // 0x0
/// public int AllocSizeBytes; // 0x4 }` (`dump.cs`, TypeDefIndex 24382).
#[derive(Clone, Copy, Debug)]
pub struct BufferPatch<'a> {
    /// `BufferPatchRecord.ChunkOffset` @ 0x0: the byte offset, counted from `Chunk.kBufferOffset`
    /// inside the chunk block, of the `DynamicBuffer` whose contents follow. The chunk holds a
    /// null pointer there; the bytes are the buffer's elements.
    pub chunk_offset: i32,
    /// `BufferPatchRecord.AllocSizeBytes` @ 0x4: the element size times the buffer's element
    /// count, which is how many bytes of `data` follow.
    pub alloc_size_bytes: i32,
    /// The `alloc_size_bytes` bytes that follow the record in the stream.
    pub data: &'a [u8],
}

/// The `EnabledBits` stream of one scene file.
///
/// The walk order is the writer's: archetypes in `Archetypes`-stream order, the chunks of each
/// archetype in `Chunks`-stream order, and the `TypesCount` component slots of each chunk in
/// order. `chunk_counts` and the `Archetypes` stream together give the offset of any one unit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SceneEnabledBits {
    /// How many chunk blocks each archetype of the `Archetypes` stream owns.
    pub chunk_counts: Vec<usize>,
    /// Component slots per archetype — `Archetype.TypesCount`, one more than the slots the
    /// `Archetypes` stream lists for it, because those leave out `Types[0]`.
    pub component_counts: Vec<usize>,
    /// One `v128` per (chunk, component) unit, in stream order.
    pub masks: Vec<ChunkEnabledMask>,
    /// The `ComponentEnabledBitsHierarchicalData` entry per unit, in the same order: how many of
    /// that chunk's entities the component is disabled for, so `disabled_counts[i]` plus the set
    /// bits of `masks[i]` is the chunk's entity count wherever the store keeps the pair live.
    pub hierarchical: Vec<i32>,
}

impl SceneEnabledBits {
    /// The number of (chunk, component) units the stream carries.
    pub fn units(&self) -> usize {
        self.masks.len()
    }

    /// The mask for one component slot of one chunk of one archetype.
    ///
    /// `slot` counts the archetype's `TypesCount` slots, so slot 0 is the `SharedComponentData`
    /// entry the `Archetypes` stream leaves out and `slot - 1` indexes its `types` list.
    pub fn mask_for(&self, archetype: usize, chunk_ordinal: usize, slot: usize) -> Option<ChunkEnabledMask> {
        let chunks = *self.chunk_counts.get(archetype)?;
        let components = *self.component_counts.get(archetype)?;
        if chunk_ordinal >= chunks || slot >= components {
            return None;
        }
        let base: usize =
            (0..archetype).map(|i| self.chunk_counts[i] * self.component_counts[i]).sum();
        self.masks.get(base + chunk_ordinal * components + slot).copied()
    }
}

/// One component's enable bitmask for one chunk: the `v128` `ArchetypeChunkData` keeps per
/// (chunk, component) pair, `ComponentEnabledBitsSizePerComponentInChunk = 16` bytes.
///
/// src: `ArchetypeChunkData.GetComponentEnabledMaskArrayForChunk` @ 0x911D5E0 and
/// `GetEnabledArrayForTypeInChunk` @ 0x911D6A0, which wrap one of these in an `UnsafeBitArray` of
/// the chunk's entity count. Bit `i` is entity `i` of that chunk: set means the component is
/// enabled for it. Measured on the install, no shipped mask sets a bit at or above its chunk's
/// `CountForSerialization`, and every mask is either empty or full — no chunk has a component
/// enabled for only some of its entities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChunkEnabledMask {
    /// The low 64 bits, as the little-endian `v128` holds them.
    pub low: u64,
    /// The high 64 bits.
    pub high: u64,
}

impl ChunkEnabledMask {
    /// Whether entity `index` of the chunk has this component enabled.
    pub fn is_enabled(&self, index: i32) -> bool {
        let bit = u64::try_from(index).ok();
        match bit {
            Some(b) if b < 64 => self.low >> b & 1 != 0,
            Some(b) if b < 128 => self.high >> (b - 64) & 1 != 0,
            _ => false,
        }
    }

    /// How many of the 128 bits are set.
    pub fn enabled_count(&self) -> u32 {
        self.low.count_ones() + self.high.count_ones()
    }

    /// The 16 bytes as the stream stores them.
    pub fn to_bytes(self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[..8].copy_from_slice(&self.low.to_le_bytes());
        out[8..].copy_from_slice(&self.high.to_le_bytes());
        out
    }
}

/// The `ComponentSize` the managed half of the `SharedAndManagedComponents` stream stores for a
/// managed object: the writer puts the two dwords `0xffffffff, 0` where an unmanaged record puts a
/// byte count, so the field reads back as `-1` and the value's real length comes out of
/// `ManagedObjectBinaryReader` instead.
///
/// src: `SerializeUtility.WriteSharedAndManagedComponents` @ 0x8fea7c0, which fills
/// `SharedComponentRecord.ComponentSize` with `0xffffffff` and the four bytes after it with zero.
/// Measured on the install: all 903 managed shared component records across 1183 files carry exactly
/// `-1` there, and no record anywhere in the stream — managed or unmanaged — has a non-zero value in
/// the four padding bytes.
pub const SHARED_COMPONENT_MANAGED_SIZE: i32 = -1;

/// One `SerializeUtility.SharedComponentRecord` as the stream stores it: 16 bytes.
///
/// src: `internal struct SerializeUtility.SharedComponentRecord { public ulong StableTypeHash; //
/// 0x0 public int ComponentSize; // 0x8 }` (`dump.cs`, TypeDefIndex 24384).
/// `ReadSharedComponentMetadata` @ 0x8fecd10 reads `int count` and then `count * 16` bytes straight
/// into a `NativeArray<SharedComponentRecord>`, which is the whole of what the stream holds here.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SharedComponentRecord {
    /// `SharedComponentRecord.StableTypeHash` @ 0x0, resolved to a live `TypeIndex` by
    /// `TypeManager.GetTypeIndexFromStableTypeHash` @ 0x90555d0 on load.
    pub stable_type_hash: u64,
    /// `SharedComponentRecord.ComponentSize` @ 0x8: the bytes this component's value occupies in the
    /// value buffer that follows the records, or `SHARED_COMPONENT_MANAGED_SIZE` for a managed
    /// object, whose length the managed object reader determines instead.
    pub component_size: i32,
}

/// The `SharedAndManagedComponents` stream of one scene file.
#[derive(Clone, Debug, Default)]
pub struct SceneSharedComponents<'a> {
    /// `sharedComponentArrays`: one entry per shared component slot the file's chunks use, holding
    /// the 1-based index of the serialized shared component that slot maps to, or zero.
    ///
    /// src: `SerializeUtility.BuildSharedComponents` @ 0x8fe87d0 splits the shared component indices
    /// it collects out of the archetype array into the unmanaged and managed groups, numbering each
    /// as it goes, and `EndDeserializeWorld` @ 0x8fe34f0 reads this array into the
    /// `NativeList<int> sharedComponentRemap` that `ReadUnmanagedSharedComponents` @ 0x8febba0 and
    /// `ReadSharedComponents` @ 0x8fec610 then append to, once per shared component they insert.
    ///
    /// Measured on the install: 82485 entries over 1183 files, values never above 39, and in every
    /// file the set of non-zero entries is exactly `1..=unmanaged + managed` — every serialized
    /// shared component is referenced and nothing points outside the list.
    pub shared_component_remap: Vec<i32>,
    /// The unmanaged shared component values, in stream order, each carved out of the value buffer by
    /// its own `ComponentSize`.
    pub unmanaged: Vec<UnmanagedSharedComponent<'a>>,
    /// The managed shared components, in stream order. Their objects sit at the front of
    /// `managed_blob`, one after another, and only `ManagedObjectBinaryReader` can say where each
    /// one ends.
    pub managed_shared_records: Vec<SharedComponentRecord>,
    /// The managed object blob: the `managed_shared_records.len()` shared component objects first,
    /// then `managed_component_count` pairs of `ulong` stable type hash plus object.
    ///
    /// src: `SerializeUtility.WriteSharedAndManagedComponents` @ 0x8fea7c0 appends the hashes of the
    /// per-entity managed components with `FUN_180035cb0` into the same `UnsafeAppendBuffer` it wrote
    /// the shared objects into; `ReadManagedSharedComponents` @ 0x8febfd0 reads the shared objects off
    /// the front via `ReadSharedComponents` @ 0x8fec610 and then loops `managedComponentCount` times
    /// over `ulong hash` plus `ManagedObjectBinaryReader.ReadObject`.
    ///
    /// Measured on the install: in the 771 files with no managed shared component, the `ulong` at the
    /// front of this blob is a stable type hash that file's `Archetypes` stream also lists.
    pub managed_blob: &'a [u8],
    /// How many per-entity managed components `managed_blob` carries after the shared objects.
    /// `WriteChunks` @ 0x8fe65e0 numbers these as it rewrites each chunk's managed component slots,
    /// which is why the chunk blocks hold an index where the live chunk held an object reference.
    pub managed_component_count: i32,
}

impl SceneSharedComponents<'_> {
    /// How many shared components the stream serializes: unmanaged and managed together. This is the
    /// count the non-zero `shared_component_remap` entries index into.
    pub fn serialized_shared_component_count(&self) -> usize {
        self.unmanaged.len() + self.managed_shared_records.len()
    }

    /// The bytes of unmanaged shared component value data the stream carries.
    pub fn total_unmanaged_bytes(&self) -> usize {
        self.unmanaged.iter().map(|c| c.data.len()).sum()
    }
}

/// One unmanaged shared component: its record and the value bytes the record's size carves out of
/// the value buffer.
#[derive(Clone, Copy, Debug)]
pub struct UnmanagedSharedComponent<'a> {
    /// `SharedComponentRecord.StableTypeHash` for this component type.
    pub stable_type_hash: u64,
    /// `SharedComponentRecord.ComponentSize`: how many bytes of `data` the stream holds for it.
    pub component_size: i32,
    /// The value itself, at the running offset `ReadUnmanagedSharedComponents` @ 0x8febba0 keeps.
    pub data: &'a [u8],
}

/// The `Unity.Entities.BlobAssetBatch` front of the `BlobAssets` stream: 16 bytes.
///
/// src: `internal struct BlobAssetBatch { private int TotalDataSize; // 0x0 private int
/// BlobAssetHeaderCount; // 0x4 private int RefCount; // 0x8 private int Padding; // 0xc }`
/// (`dump.cs`, TypeDefIndex 23452), filled by `CreateForSerialize` @ 0x9070490 and checked by
/// `CreateFromMemory` @ 0x90704b0.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BlobAssetBatchHeader {
    /// `TotalDataSize`: the size of the whole batch, which the runtime requires to equal the
    /// size of the buffer it was handed — the node's `DataSize`.
    pub total_data_size: i32,
    /// `BlobAssetHeaderCount`: how many `BlobAssetHeader` records follow.
    pub blob_asset_header_count: i32,
    /// `RefCount`. `CreateForSerialize` stores 1 and `CreateFromMemory` rejects anything else,
    /// so the serialized batch always carries the batch's own single reference.
    ///
    /// Measured on the install: 1 in all 1183 files that carry the stream.
    pub ref_count: i32,
    /// `Padding`, zero in every shipped batch.
    pub padding: i32,
}

/// The `Unity.Entities.BlobAssetHeader` in front of one blob: 32 bytes.
///
/// src: `internal struct BlobAssetHeader { public void* ValidationPtr; // 0x0 public int
/// Length; // 0x8 public AllocatorManager.AllocatorHandle Allocator; // 0xc public ulong Hash;
/// // 0x10 private ulong Padding; // 0x18 }` (`dump.cs`, TypeDefIndex 23454), filled by
/// `CreateForSerialize` @ 0x90709c0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlobAssetRecordHeader {
    /// `ValidationPtr`. The writer stores zero; `CreateFromMemory` @ 0x90704b0 overwrites it
    /// in place with the address of the bytes that follow the header.
    pub validation_ptr: u64,
    /// `Length`: the blob's byte length, rounded up to `BLOB_ASSET_ALIGNMENT`.
    pub length: i32,
    /// `Allocator`. `CreateForSerialize` stores 1 — an allocator handle that owns nothing,
    /// because the batch owns the memory — and `CreateFromMemory` rejects anything else.
    pub allocator: u32,
    /// `Hash`: the blob's content hash, the key `BlobAssetStore` and the persistence path
    /// match blobs by.
    pub hash: u64,
    /// `Padding`, zero in every shipped blob.
    pub padding: u64,
}

/// The `BlobAssets` stream of one scene file.
#[derive(Clone, Debug, Default)]
pub struct SceneBlobAssets<'a> {
    /// The 16-byte `BlobAssetBatch` front of the stream.
    pub header: BlobAssetBatchHeader,
    /// The blobs, in stream order. `header.blob_asset_header_count` is their count.
    pub blobs: Vec<SceneBlobAsset<'a>>,
}

/// One blob of the `BlobAssets` stream: its header and the bytes after it.
#[derive(Clone, Copy, Debug)]
pub struct SceneBlobAsset<'a> {
    /// Position of this blob in the batch, counting from zero.
    pub index: usize,
    /// Byte offset of `data` from the start of the stream. This is the value a serialized
    /// `BlobAssetReferenceData` holds: `CalculateBlobAssetBatchTotalSize` @ 0x8fe81f0 records
    /// exactly this number, and `ReadUnmanagedSharedComponents` @ 0x8febba0 adds the batch base
    /// to it to rebuild the pointer.
    pub offset: usize,
    /// `BlobAssetHeader.Hash`.
    pub hash: u64,
    /// `BlobAssetHeader.Length`: how many bytes of `data` the batch holds for it.
    pub length: i32,
    /// The blob itself.
    pub data: &'a [u8],
}

impl SceneBlobAssets<'_> {
    /// The blob whose payload starts at `offset`, the way a serialized `BlobAssetReferenceData`
    /// names one. `-1` is the null reference and maps to `None`, as
    /// `ReadUnmanagedSharedComponents` @ 0x8febba0 treats it.
    pub fn find_by_offset(&self, offset: i32) -> Option<&SceneBlobAsset<'_>> {
        if offset < 0 {
            return None;
        }
        let offset = offset as usize;
        self.blobs.iter().find(|b| b.offset == offset)
    }

    /// The payload offsets of every blob, in stream order — the `blobAssetOffsets` array
    /// `WriteBlobAssetNode` @ 0x8fe7ad0 hands back to the writer of the chunk data.
    pub fn offsets(&self) -> impl Iterator<Item = usize> + '_ {
        self.blobs.iter().map(|b| b.offset)
    }

    /// Bytes of blob payload the batch carries, not counting any header.
    pub fn total_blob_bytes(&self) -> usize {
        self.blobs.iter().map(|b| b.data.len()).sum()
    }
}

/// `BlobAssetBatch.CreateForSerialize` @ 0x9070490 stores 1 in `RefCount` and
/// `CreateFromMemory` @ 0x90704b0 requires it.
pub const BLOB_ASSET_BATCH_REF_COUNT: i32 = 1;
/// `BlobAssetHeader.CreateForSerialize` @ 0x90709c0 stores 1 in `Allocator` and
/// `BlobAssetBatch.CreateFromMemory` @ 0x90704b0 requires it.
pub const BLOB_ASSET_HEADER_ALLOCATOR: u32 = 1;
/// The alignment `CalculateBlobAssetBatchTotalSize` @ 0x8fe81f0 rounds every blob up to —
/// `length + 0xf & ~0xf` — and the alignment blob array contents need.
pub const BLOB_ASSET_ALIGNMENT: i32 = 16;

/// Read the 16 bytes of one `SerializeUtility.SharedComponentRecord`, including the four bytes the
/// struct pads out after `ComponentSize`.
fn read_shared_component_record(c: &mut Cursor<'_>, index: i32) -> Result<SharedComponentRecord> {
    let stable_type_hash = c.u64()?;
    let component_size = c.i32()?;
    let pad = c.u32()?;
    if pad != 0 {
        bail!("shared component record {index} carries {pad:#010x} in its four padding bytes");
    }
    Ok(SharedComponentRecord { stable_type_hash, component_size })
}

/// A bounds-checked little-endian cursor over one node's data blob.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
    what: &'static str,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8], what: &'static str) -> Self {
        Self { bytes, at: 0, what }
    }

    fn need(&mut self, n: usize) -> Result<()> {
        if self.at + n > self.bytes.len() {
            bail!("{} ends {} bytes short at offset {:#x}", self.what, self.at + n - self.bytes.len(), self.at);
        }
        Ok(())
    }

    fn i32(&mut self) -> Result<i32> {
        self.need(4)?;
        let v = i32_at(self.bytes, self.at);
        self.at += 4;
        Ok(v)
    }

    fn u32(&mut self) -> Result<u32> {
        self.need(4)?;
        let v = u32::from_le_bytes([
            self.bytes[self.at],
            self.bytes[self.at + 1],
            self.bytes[self.at + 2],
            self.bytes[self.at + 3],
        ]);
        self.at += 4;
        Ok(v)
    }

    fn u64(&mut self) -> Result<u64> {
        self.need(8)?;
        let v = u64_at(self.bytes, self.at);
        self.at += 8;
        Ok(v)
    }

    /// Take `n` bytes and advance over them.
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        self.need(n)?;
        let v = &self.bytes[self.at..self.at + n];
        self.at += n;
        Ok(v)
    }

    fn at_end(&self) -> bool {
        self.at == self.bytes.len()
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }
}

/// Name a node from its hash and its trailing bytes, or keep both when they do not settle it.
fn classify(header: &NodeHeader, tail: &[u8]) -> NodeKind {
    match header.node_type_hash {
        NODE_TYPE_FOLDER => NodeKind::Folder,
        h => {
            let extra = if tail.len() >= 4 {
                Some(i32::from_le_bytes([tail[0], tail[1], tail[2], tail[3]]))
            } else {
                None
            };
            if h == NODE_TYPE_RAW_DATA {
                NodeKind::RevisionedRawData { revision: extra.unwrap_or(0), stream: stream_of_id(&header.id) }
            } else {
                NodeKind::Unknown { hash: h, extra }
            }
        }
    }
}
