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
const LINKED_GROUP: u64 = 0xbea7_9ca4_5e4b_e521;
const SCALARS: [(&str, u64, usize, &[usize]); 4] = [
    ("ProjectM.Buff", 0x5c06_8978_8602_b4e5, 32, &[8]),
    ("ProjectM.EntityOwner", 0x4a55_fea9_09ee_9afc, 8, &[0]),
    ("ProjectM.EntityInput", 0xcf49_a498_d747_4572, 96, &[48]),
    ("ProjectM.MapCollision", 0x179e_efa3_019c_a077, 20, &[]),
];
const BUFFERS: [(&str, u64, usize, &[usize]); 2] = [
    ("Unity.Entities.LinkedEntityGroup", LINKED_GROUP, 8, &[0]),
    (
        "ProjectM.MegaStaticDestroyedBuffer",
        0x42f4_6c39_941c_cb09,
        16,
        &[],
    ),
];

pub struct RestoredWorldProjection {
    pub store: EntityComponentStore,
    pub types: TypeManager,
    pub prefabs: Vec<PrefabReference>,
    pub bindings: SavedEntityBindings,
    pub copied_scalar_values: usize,
    pub copied_buffer_rows: usize,
    pub copied_buffer_elements: usize,
    pub prefab_linked_groups: usize,
    pub cleared_references: usize,
}

impl RestoredWorldProjection {
    /// Allocates real ECS handles for the prefab topology and every saved job row,
    /// then restores the validated scalar and buffer subset.
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
        let mut buffers = FxHashMap::default();
        let mut linked_group = None;
        for (name, hash, stride, offsets) in BUFFERS {
            let component = snapshot
                .header()
                .persistent_component_named(name)
                .context("missing saved buffer metadata")?;
            let layout = layouts.get(hash).context("missing buffer layout")?;
            ensure!(
                component.stable_type_hash == hash
                    && component.component_size as usize == stride
                    && component.saved_type_index.is_buffer()
                    && layout.spec.element_size as usize == stride
                    && layout
                        .entity_offsets
                        .iter()
                        .map(|&offset| offset as usize)
                        .eq(offsets.iter().copied()),
                "buffer layout disagrees for {name}"
            );
            let ty = register(hash)?;
            if hash == LINKED_GROUP {
                linked_group = Some(ty);
            }
            buffers.insert(component.saved_type_index, (ty, stride, offsets));
        }
        let linked_group = linked_group.context("missing live linked group type")?;
        let mut store = EntityComponentStore::new(
            i32::try_from(snapshot.entity_count() + 2)?,
            SpecialTypes {
                entity_type,
                prefab: prefab_tag,
                ..Default::default()
            },
        )?;
        let root_arch = store.get_or_create_archetype(&types, &[prefab_tag, prefab_guid])?;
        let linked_root_arch =
            store.get_or_create_archetype(&types, &[prefab_tag, prefab_guid, linked_group])?;
        let child_arch = store.get_or_create_archetype(&types, &[prefab_tag])?;
        let mut prefabs = Vec::new();
        let mut guids = BTreeSet::new();
        let mut prefab_linked_groups = 0;
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
                let root = store.create_entity(if prefab.linked_entities.is_empty() {
                    root_arch
                } else {
                    linked_root_arch
                })?;
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
                let root = scene_handles[&prefab.entity];
                if !linked_entities.is_empty() {
                    let payload: Vec<u8> = linked_entities
                        .iter()
                        .flat_map(|entity| {
                            entity
                                .index
                                .to_le_bytes()
                                .into_iter()
                                .chain(entity.version.to_le_bytes())
                        })
                        .collect();
                    store.set_buffer_payload(&types, root, linked_group, &payload)?;
                    prefab_linked_groups += 1;
                }
                prefabs.push(PrefabReference {
                    prefab_guid: prefab.prefab_guid,
                    entity: root,
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
                            scalars
                                .get(&column.saved_type_index)
                                .or_else(|| buffers.get(&column.saved_type_index))
                                .map(|layout| layout.0)
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
        let mut copied_buffer_rows = 0;
        let mut copied_buffer_elements = 0;
        let mut cleared_references = 0;
        for (_, job) in snapshot.jobs() {
            for archetype in &job.archetypes {
                for chunk in &archetype.chunks {
                    for column in &chunk.columns {
                        if let Some(&(live_type, stride, offsets)) =
                            buffers.get(&column.saved_type_index)
                        {
                            let payloads =
                                column.buffers().context("expected saved buffer rows")?;
                            let mut data = Vec::new();
                            for (source, payload) in chunk.entities().zip(payloads) {
                                data.clear();
                                data.extend_from_slice(payload);
                                cleared_references +=
                                    bindings.patch_references(&mut data, stride, offsets)?;
                                store.set_buffer_payload(
                                    &types,
                                    bindings.target(source).context("missing live entity")?,
                                    live_type,
                                    &data,
                                )?;
                                copied_buffer_rows += 1;
                                copied_buffer_elements += data.len() / stride;
                            }
                            continue;
                        }
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
            copied_buffer_rows,
            copied_buffer_elements,
            prefab_linked_groups,
            cleared_references,
        })
    }
}
