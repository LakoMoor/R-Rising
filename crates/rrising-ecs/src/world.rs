//! A port-side `World` builder: instantiate decoded chunk/archetype data into an
//! [`EntityComponentStore`].
//!
//! The input structs here are plain data so this crate needs no dependency on `rrising-content`;
//! a thin adapter from the scene decoder can fill them later. Nothing here is an original function:
//! the original deserialiser (`EntityComponentStore` + `EntityRemapUtility`) is not ported, only its
//! observable end state (entities live at their stored index/version, component bytes in place).
//!
//! Limits: archetypes with shared/managed/chunk/cleanup components are refused by the store unless
//! [`World::build_with`] asks for opaque admission (layout only; values not modelled);
//! shared-component value payloads, chunk components, buffer heap payloads and entity-reference patching
//! are not handled; `Disabled`/`Prefab` flags come from the supplied [`SpecialTypes`].

use anyhow::{ensure, Context, Result};
use std::collections::HashMap;

use crate::archetype::SpecialTypes;
use crate::entity::Entity;
use crate::remap::HierarchyReport;
use crate::store::{EntityComponentStore, EntityQuery};
use crate::type_table::{TypeIndex, TypeManager};

/// One archetype: its component types (the Entity type is added/deduplicated by the store).
#[derive(Clone, Debug, Default)]
pub struct ArchetypeInput {
    pub types: Vec<TypeIndex>,
}

/// Which ids the entities of a chunk get.
#[derive(Clone, Debug)]
pub enum EntityIds {
    /// The scene stored these ids; keep index and version exactly.
    Preserved(Vec<Entity>),
    /// Allocate `n` fresh entities from the free list.
    Fresh(usize),
}

impl EntityIds {
    pub fn len(&self) -> usize {
        match self {
            Self::Preserved(v) => v.len(),
            Self::Fresh(n) => *n,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// One chunk's worth of entities of one archetype.
#[derive(Clone, Debug)]
pub struct ChunkInput {
    /// Index into [`WorldInput::archetypes`].
    pub archetype: usize,
    pub entities: EntityIds,
    /// Component bytes per type, tightly packed `entities.len() * size_of(type)` bytes (the
    /// structure-of-arrays layout a chunk stores). Types not listed stay zeroed.
    pub components: Vec<(TypeIndex, Vec<u8>)>,
    /// Scene-local shared value indices, keyed by component type. Missing entries default to zero.
    pub shared_components: Vec<(TypeIndex, i32)>,
}

#[derive(Clone, Debug, Default)]
pub struct WorldInput {
    pub archetypes: Vec<ArchetypeInput>,
    pub chunks: Vec<ChunkInput>,
}

/// A built world: the store plus the type table and the input-archetype to store-archetype map.
#[derive(Debug)]
pub struct World {
    pub store: EntityComponentStore,
    pub types: TypeManager,
    /// `archetype_map[input_index]` is the store's archetype index.
    pub archetype_map: Vec<usize>,
    /// Every instantiated entity, in input order.
    pub entities: Vec<Entity>,
    blob_field_offsets: HashMap<TypeIndex, Vec<usize>>,
    blob_address_ids: HashMap<u64, u64>,
}

impl World {
    /// Build a world from decoded data. `initial_capacity` is the starting entity table size
    /// (grown as needed).
    pub fn build(
        types: TypeManager,
        special: SpecialTypes,
        initial_capacity: i32,
        input: &WorldInput,
    ) -> Result<World> {
        Self::build_with(types, special, initial_capacity, input, false)
    }

    /// Like [`World::build`]; with `admit_opaque_special_components` archetypes holding
    /// shared/managed/chunk/cleanup components are built layout-only (see
    /// [`EntityComponentStore::set_admit_opaque_special_components`]).
    pub fn build_with(
        types: TypeManager,
        special: SpecialTypes,
        initial_capacity: i32,
        input: &WorldInput,
        admit_opaque_special_components: bool,
    ) -> Result<World> {
        let mut store = EntityComponentStore::new(initial_capacity, special)?;
        store.set_admit_opaque_special_components(admit_opaque_special_components);
        let mut archetype_map = Vec::with_capacity(input.archetypes.len());
        for a in &input.archetypes {
            archetype_map.push(store.get_or_create_archetype(&types, &a.types)?);
        }
        let mut entities = Vec::new();
        for (ci, chunk) in input.chunks.iter().enumerate() {
            ensure!(
                chunk.archetype < archetype_map.len(),
                "chunk {ci} names unknown archetype {}",
                chunk.archetype
            );
            let arch = archetype_map[chunk.archetype];
            let layout = &store.archetypes()[arch].archetype;
            let shared_types = &layout.types[layout.data.layout.first_shared_component as usize
                ..layout.data.layout.first_chunk_component as usize];
            let mut shared_values = vec![0; shared_types.len()];
            let mut seen = std::collections::HashSet::new();
            for &(ty, value) in &chunk.shared_components {
                ensure!(seen.insert(ty), "chunk {ci}: duplicate shared component");
                let slot = shared_types
                    .iter()
                    .position(|t| t.type_index == ty)
                    .with_context(|| {
                        format!("chunk {ci}: shared component absent from archetype")
                    })?;
                shared_values[slot] = value;
            }
            let count = chunk.entities.len();
            if count == 0 {
                continue;
            }
            let ids = match &chunk.entities {
                EntityIds::Preserved(ids) => {
                    store.restore_entities_with_shared(arch, ids, &shared_values)?;
                    ids.clone()
                }
                EntityIds::Fresh(n) => {
                    store.create_entities_with_shared(arch, *n, &shared_values)?
                }
            };
            for (t, data) in &chunk.components {
                let size = store.component_bytes(ids[0], *t)?.len();
                ensure!(
                    data.len() == size * count,
                    "chunk {ci}: component {:?} has {} bytes, expected {}",
                    t,
                    data.len(),
                    size * count
                );
                if size == 0 {
                    continue;
                }
                for (e, bytes) in ids.iter().zip(data.chunks_exact(size)) {
                    store.set_component_bytes(*e, *t, bytes)?;
                }
            }
            entities.extend(ids);
        }
        Ok(World {
            store,
            types,
            archetype_map,
            entities,
            blob_field_offsets: HashMap::new(),
            blob_address_ids: HashMap::new(),
        })
    }

    /// Safe liveness check: `Some(e)` when `e` names a live entity (index in range, version
    /// current, slot occupied); `None` for `Entity.Null`, out-of-range indices and stale handles.
    pub fn resolve(&self, e: Entity) -> Option<Entity> {
        if e == Entity::NULL {
            return None;
        }
        self.store.exists(e).ok()?.then_some(e)
    }

    /// Walk a `Parent`-style relation. `parent_type` is a per-entity 8-byte component whose first
    /// field is the parent `Entity`. Counts parents that are null / not live and entities whose
    /// chain never terminates (visited-set cycle detection), plus the longest chain.
    pub fn check_hierarchy(&self, parent_type: TypeIndex) -> Result<HierarchyReport> {
        let q = EntityQuery {
            all: vec![parent_type],
            include_disabled: true,
            include_prefab: true,
            ..Default::default()
        };
        let mut parent_of = std::collections::HashMap::new();
        let mut rep = HierarchyReport::default();
        for e in self.store.query_entities(&q)? {
            let b = self.store.component_bytes(e, parent_type)?;
            ensure!(b.len() >= 8, "parent component is {} bytes", b.len());
            let p = Entity::new(
                i32::from_le_bytes(b[0..4].try_into()?),
                i32::from_le_bytes(b[4..8].try_into()?),
            );
            rep.with_parent += 1;
            if p == Entity::NULL {
                rep.null_parent += 1;
            } else if self.resolve(p).is_none() {
                rep.dangling_parent += 1;
            } else {
                parent_of.insert(e.index, p.index);
            }
        }
        // depth memo: 0 = unknown, usize::MAX = on the current path / cyclic
        let mut depth: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
        for &start in parent_of.keys() {
            let mut path = Vec::new();
            let mut cur = start;
            let result = loop {
                if let Some(&d) = depth.get(&cur) {
                    break d;
                }
                if path.contains(&cur) {
                    break usize::MAX;
                }
                match parent_of.get(&cur) {
                    Some(&n) => {
                        path.push(cur);
                        cur = n;
                    }
                    None => {
                        depth.insert(cur, 0);
                        break 0;
                    }
                }
            };
            let mut d = result;
            for &n in path.iter().rev() {
                d = if d == usize::MAX { d } else { d + 1 };
                depth.insert(n, d);
            }
        }
        for &d in depth.values() {
            if d == usize::MAX {
                rep.cyclic += 1;
            } else {
                rep.max_depth = rep.max_depth.max(d);
            }
        }
        Ok(rep)
    }

    /// Number of matching rows after applying enable masks.
    pub fn query_count(&self, q: &EntityQuery) -> usize {
        self.store
            .query_entity_count(q)
            .expect("valid query chunk metadata")
    }
    /// Register known blob field layouts and stable identifiers for the diagnostic fingerprint.
    pub fn register_blob_fingerprint(
        &mut self,
        ty: TypeIndex,
        offsets: Vec<usize>,
        addresses: &[(u64, u64)],
    ) -> Result<()> {
        let info = self.types.type_info(ty).context("unknown type")?;
        let size = usize::try_from(if ty.is_buffer() {
            info.element_size
        } else {
            info.size_in_chunk
        })?;
        ensure!(
            offsets.windows(2).all(|w| w[0] + 8 <= w[1])
                && offsets
                    .iter()
                    .all(|&o| o.checked_add(8).is_some_and(|end| end <= size)),
            "invalid blob field offsets"
        );
        for &(address, id) in addresses {
            ensure!(
                address != 0 && id != 0,
                "invalid blob address or identifier"
            );
            if let Some(previous) = self.blob_address_ids.insert(address, id) {
                ensure!(previous == id, "conflicting blob identifier");
            }
        }
        self.blob_field_offsets.insert(ty, offsets);
        Ok(())
    }

    /// Diagnostic fingerprint of component data, including owned buffer contents. Registered blob
    /// addresses are replaced by scene-local identifiers; unknown addresses remain raw bytes.
    pub fn state_hash(&self) -> u64 {
        let mut h = Fnv::new();
        for sa in self.store.archetypes() {
            h.write(&(sa.chunk_data.chunk_indices().len() as u64).to_le_bytes());
            for &c in sa.chunk_data.chunk_indices() {
                let Some(ch) = self.store.chunk(c) else {
                    continue;
                };
                h.write(&ch.count.to_le_bytes());
                for t in &sa.archetype.types {
                    let ty = t.type_index;
                    if ty.is_buffer() {
                        if let Ok(entities) = self.store.chunk_entities(c) {
                            let info = self.types.type_info(ty).expect("registered type");
                            for entity in entities {
                                if let Ok(payload) =
                                    self.store.buffer_payload(&self.types, entity, ty)
                                {
                                    h.write(&(payload.len() as u64).to_le_bytes());
                                    self.hash_blob_fields(
                                        &mut h,
                                        ty,
                                        payload,
                                        info.element_size as usize,
                                    );
                                } else if let Ok(bytes) = self.store.component_bytes(entity, ty) {
                                    h.write(bytes);
                                }
                            }
                        }
                    } else if let Ok(bytes) = self.store.chunk_component_bytes(c, ty) {
                        let stride = self
                            .types
                            .type_info(ty)
                            .map_or(0, |info| info.size_in_chunk as usize);
                        self.hash_blob_fields(&mut h, ty, bytes, stride);
                    }
                }
            }
        }
        h.finish()
    }

    fn hash_blob_fields(&self, h: &mut Fnv, ty: TypeIndex, bytes: &[u8], stride: usize) {
        let Some(offsets) = self.blob_field_offsets.get(&ty) else {
            h.write(bytes);
            return;
        };
        if stride == 0 {
            h.write(bytes);
            return;
        }
        for element in bytes.chunks_exact(stride) {
            let mut start = 0;
            for &at in offsets {
                h.write(&element[start..at]);
                let address =
                    u64::from_le_bytes(element[at..at + 8].try_into().expect("validated offset"));
                h.write(
                    &self
                        .blob_address_ids
                        .get(&address)
                        .copied()
                        .unwrap_or(address)
                        .to_le_bytes(),
                );
                start = at + 8;
            }
            h.write(&element[start..]);
        }
    }

    fn all_chunks_with(&self, t: TypeIndex) -> Vec<crate::chunk::ChunkIndex> {
        self.store.query_chunks(&EntityQuery {
            all: vec![t],
            include_disabled: true,
            include_prefab: true,
            ..Default::default()
        })
    }

    /// FNV-1a 64 over one component's bytes in every chunk that has it (query order).
    pub fn component_hash(&self, t: TypeIndex) -> u64 {
        let mut h = Fnv::new();
        for c in self.all_chunks_with(t) {
            if let Ok(b) = self.store.chunk_component_bytes(c, t) {
                h.write(b);
            }
        }
        h.finish()
    }

    /// Change version of one component per chunk, in the same order as [`Self::component_hash`].
    pub fn component_versions(&self, t: TypeIndex) -> Vec<u32> {
        self.all_chunks_with(t)
            .into_iter()
            .filter_map(|c| self.store.chunk_change_version(c, t).ok())
            .collect()
    }
}

/// FNV-1a 64.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn write(&mut self, b: &[u8]) {
        for &x in b {
            self.0 = (self.0 ^ u64::from(x)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn finish(&self) -> u64 {
        self.0
    }
}
