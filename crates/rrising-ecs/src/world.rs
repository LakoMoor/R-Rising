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
//! shared-component values, chunk components, buffer heap payloads and entity-reference patching
//! are not handled; `Disabled`/`Prefab` flags come from the supplied [`SpecialTypes`].

use anyhow::{ensure, Result};

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
            let count = chunk.entities.len();
            if count == 0 {
                continue;
            }
            let ids = match &chunk.entities {
                EntityIds::Preserved(ids) => {
                    store.restore_entities(arch, ids)?;
                    ids.clone()
                }
                EntityIds::Fresh(n) => store.create_entities(arch, *n)?,
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

    /// Number of entities matched by the query (sum of matching chunk counts).
    pub fn query_count(&self, q: &EntityQuery) -> usize {
        self.store
            .query_chunks(q)
            .into_iter()
            .filter_map(|c| self.store.chunk(c))
            .map(|c| c.count as usize)
            .sum()
    }
}
