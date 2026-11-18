//! First live ECS projection of original server scenes and save rows.

use std::collections::BTreeSet;

use anyhow::{ensure, Context, Result};
use rrising_ecs::{Entity, EntityComponentStore, SpecialTypes, TypeIndex, TypeManager, TypeSpec};
use rustc_hash::FxHashMap;

use crate::{
    persistence::{PrefabReference, SavedEntityBindings, SavedSnapshot},
    prefab_catalog::ScenePrefabCatalog,
    type_layouts::{TypeLayouts, ENTITY_HASH},
};

const PREFAB_GUID: u64 = 0xb181_3197_8820_0ae1;
const PREFAB_TAG: u64 = 0x616a_36e3_c0cf_ef8c;
const SCALARS: [(&str, u64, usize, &[usize]); 4] = [
    ("ProjectM.Buff", 0x5c06_8978_8602_b4e5, 32, &[8]),
    ("ProjectM.EntityOwner", 0x4a55_fea9_09ee_9afc, 8, &[0]),
    ("ProjectM.EntityInput", 0xcf49_a498_d747_4572, 96, &[48]),
    ("ProjectM.MapCollision", 0x179e_efa3_019c_a077, 20, &[]),
];

pub struct RestoredWorldProjection {
    pub store: EntityComponentStore,
    pub types: TypeManager,
    pub prefabs: Vec<PrefabReference>,
    pub bindings: SavedEntityBindings,
    pub copied_scalar_values: usize,
    pub cleared_references: usize,
}

impl RestoredWorldProjection {
    /// Allocates real ECS handles for the prefab topology and every saved job row,
    /// then restores the validated scalar subset. Other component state remains absent.
    pub fn build(
        snapshot: &SavedSnapshot<'_>,
        layouts: &TypeLayouts,
        catalogs: &[ScenePrefabCatalog],
    ) -> Result<Self> {
        let mut types = TypeManager::default();
        types.register(TypeSpec::tag("<reserved null>"))?;
        let mut register = |hash| -> Result<TypeIndex> {
            types.register(
                layouts
                    .get(hash)
                    .context("missing server component layout")?
                    .spec
                    .clone(),
            )
        };
        let entity_type = register(ENTITY_HASH)?;
        let prefab_tag = register(PREFAB_TAG)?;
        let prefab_guid = register(PREFAB_GUID)?;
        let mut scalars = FxHashMap::default();
        for (name, hash, stride, offsets) in SCALARS {
            let component = snapshot
                .header()
                .persistent_component_named(name)
                .context("missing saved scalar metadata")?;
            ensure!(
                component.stable_type_hash == hash
                    && component.component_size as usize == stride
                    && !component.saved_type_index.is_buffer(),
                "saved layout disagrees for {name}"
            );
            let layout = layouts.get(hash).context("missing scalar layout")?;
            ensure!(
                layout.spec.size_in_chunk as usize == stride
                    && layout
                        .entity_offsets
                        .iter()
                        .map(|&offset| offset as usize)
                        .eq(offsets.iter().copied()),
                "server layout disagrees for {name}"
            );
            scalars.insert(
                component.saved_type_index,
                (register(hash)?, stride, offsets),
            );
        }
        let mut store = EntityComponentStore::new(
            i32::try_from(snapshot.entity_count() + 2)?,
            SpecialTypes {
                entity_type,
                prefab: prefab_tag,
                ..Default::default()
            },
        )?;
        let root_arch = store.get_or_create_archetype(&types, &[prefab_tag, prefab_guid])?;
        let child_arch = store.get_or_create_archetype(&types, &[prefab_tag])?;
        let mut prefabs = Vec::new();
        let mut guids = BTreeSet::new();
        for catalog in catalogs {
            ensure!(
                catalog.unresolved_rows == 0,
                "incomplete scene prefab catalog"
            );
            let mut scene_handles = FxHashMap::default();
            for prefab in &catalog.prefabs {
                ensure!(
                    guids.insert(prefab.prefab_guid),
                    "duplicate prefab GUID {}",
                    prefab.prefab_guid
                );
                let root = store.create_entity(root_arch)?;
                ensure!(
                    scene_handles.insert(prefab.entity, root).is_none(),
                    "duplicate prefab root"
                );
                store.set_component_bytes(root, prefab_guid, &prefab.prefab_guid.to_le_bytes())?;
            }
            for prefab in &catalog.prefabs {
                let mut linked_entities = Vec::with_capacity(prefab.linked_entities.len());
                for &source in &prefab.linked_entities {
                    let current = match scene_handles.entry(source) {
                        std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            *entry.insert(store.create_entity(child_arch)?)
                        }
                    };
                    linked_entities.push(current);
                }
                prefabs.push(PrefabReference {
                    prefab_guid: prefab.prefab_guid,
                    entity: scene_handles[&prefab.entity],
                    linked_entities,
                });
            }
        }
        let mut fresh = Vec::with_capacity(snapshot.entity_count());
        for (_, job) in snapshot.jobs() {
            for archetype in &job.archetypes {
                for chunk in &archetype.chunks {
                    let projected: Vec<_> = chunk
                        .columns
                        .iter()
                        .filter_map(|column| {
                            scalars.get(&column.saved_type_index).map(|layout| layout.0)
                        })
                        .collect();
                    let arch = store.get_or_create_archetype(&types, &projected)?;
                    fresh.extend(store.create_entities(arch, chunk.entities().len())?);
                }
            }
        }
        let bindings = snapshot.bind_entities(fresh, &prefabs)?;
        ensure!(
            bindings.issues().is_empty(),
            "incomplete prefab/job bindings: {:?}",
            bindings.issues().first()
        );
        let mut copied_scalar_values = 0;
        let mut cleared_references = 0;
        for (_, job) in snapshot.jobs() {
            for archetype in &job.archetypes {
                for chunk in &archetype.chunks {
                    for column in &chunk.columns {
                        let Some(&(live_type, stride, offsets)) =
                            scalars.get(&column.saved_type_index)
                        else {
                            continue;
                        };
                        let mut data = vec![0; stride];
                        for (row, source) in chunk.entities().enumerate() {
                            data.copy_from_slice(
                                column.scalar(row).context("missing saved scalar row")?,
                            );
                            cleared_references +=
                                bindings.patch_references(&mut data, stride, offsets)?;
                            store.set_component_bytes(
                                bindings.target(source).context("missing live entity")?,
                                live_type,
                                &data,
                            )?;
                            copied_scalar_values += 1;
                        }
                    }
                }
            }
        }
        ensure!(
            bindings.target(Entity::NULL) == Some(Entity::NULL),
            "lost null binding"
        );
        Ok(Self {
            store,
            types,
            prefabs,
            bindings,
            copied_scalar_values,
            cleared_references,
        })
    }
}
