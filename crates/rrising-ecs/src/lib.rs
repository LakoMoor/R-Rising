//! `rrising-ecs` — the port of the `Unity.Entities` runtime the original's gameplay runs on.
//!
//! Source of truth: the decompiled bodies of the shipped `GameAssembly.dll` in
//! `../evidence/decompiled/Unity.Entities.dll.c` (Ghidra, function names from Il2CppDumper) and the
//! field layouts in `dump.cs`. Every ported item carries a `// src:` comment naming the body it was
//! translated from. Where the corpus has no body for a member, the member is left out and listed
//! here rather than invented:
//!
//! - `Entity.ToString` and `Entity.ToFixedString` (text renderings of the handle).
//! - `BufferHeader.PatchAfterCloningChunk` and the chunk/section layout it belongs to.
//! - `Archetype.Create` / `Archetype.CalculateSize` / `Archetype.Free`. The corpus has the archetype
//!   *getters* and `EntityComponentStore.CreateArchetype`, which is where the layout arithmetic
//!   actually lives, so `archetype.rs` is translated from those; the allocation bookkeeping around
//!   them is not ported.
//! - `Chunk`'s own members and `ChunkIndex.GetPtr` @ 0x91212b0. `chunk.rs` ports the layout the scene
//!   reader and every component accessor depend on, named after the `dump.cs` properties.
//! - `ArchetypeChunkData` (the per-archetype parallel arrays of change versions, shared component
//!   values and component-enabled bits). The corpus has 40-odd bodies for it; they are the next
//!   module.
//! - `EntityComponentStore` is partly ported in `store.rs` (entity table, create/destroy, minimal
//!   query; limits listed there). Still absent: `EntityQueryData`, `MatchingArchetype`, `SystemState` and the
//!   world update loop.
//!
//! One corpus fact worth knowing when reading the `// src:` comments: IL2CPP merges identical
//! methods, so `Entity.op_Equality` and `TrailSystemID.op_Equality` are the *same compiled
//! function* at one address. The dump lists both names; the address map keeps one. Those bodies are
//! cited by address, and the comment says which other name the corpus filed them under.

pub mod archetype;
pub mod buffer;
pub mod change_version;
pub mod chunk;
pub mod chunk_data;
pub mod entity;
pub mod hash128;
pub mod remap;
pub mod schedule;
pub mod store;
pub mod transform;
pub mod type_table;
pub mod world;

pub use archetype::{
    create_archetype_data, Archetype, ArchetypeData, ArchetypeFlags, ArchetypeLayout,
    BufferEntityPatchInfo, EntityPatchInfo, SpecialTypes, K_CHUNK_BUFFER_SIZE,
    K_MAXIMUM_ENTITIES_PER_CHUNK, K_QUERY_MASK_BYTES, NONE,
};
pub use buffer::{BufferHeader, TrashMode, ELEMENT_OFFSET, K_MINIMUM_CAPACITY};
pub use chunk::{
    Chunk, ChunkFlags, ChunkHeader, ChunkIndex, K_BUFFER_OFFSET, K_BUFFER_SIZE, K_CHUNK_SIZE,
    K_HEADER_FIELDS_SIZE, K_SERIALIZED_HEADER_SIZE,
};
pub use entity::Entity;
pub use hash128::{Hash128, K_GUID_STRING_LENGTH};
pub use type_table::{
    AccessMode, ComponentType, ComponentTypeInArchetype, TypeCategory, TypeIndex, TypeInfo,
    TypeManager, TypeSpec, K_MAX_TYPE_COUNT,
};

pub use chunk_data::{ArchetypeChunkData, ChunkRemoval};

pub mod render_mesh;
pub use render_mesh::MaterialMeshInfo;

pub use store::{
    EntityComponentStore, EntityInChunk, EntityQuery, StoredArchetype, K_MAX_ENTITIES_PER_WORLD,
};

pub use remap::{
    classify_elements, remap_elements, EntityRemapTable, HierarchyReport, RefClass, RefCounts,
};
pub use schedule::{FrameContext, Schedule, System};
pub use world::{ArchetypeInput, ChunkInput, EntityIds, World, WorldInput};
