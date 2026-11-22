//! First live ECS projection of original server scenes and save rows.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{ensure, Context, Result};
use rrising_ecs::{
    Entity, EntityComponentStore, SpecialTypes, TypeCategory, TypeIndex, TypeManager, TypeSpec,
};
use rustc_hash::FxHashMap;

use crate::{
    persistence::{MegaStaticRestorePlan, PrefabReference, SavedEntityBindings, SavedSnapshot},
    prefab_catalog::{
        LinkedPrefabTemplate, PrefabBlobPayload, PrefabRootBlobs, ScenePrefabCatalog,
    },
    static_transforms::StaticTransformCatalog,
    type_layouts::{TypeLayouts, ENTITY_HASH, ROTATION_HASH, TRANSLATION_HASH},
};

const PREFAB_GUID: u64 = 0xb181_3197_8820_0ae1;
const PREFAB_TAG: u64 = 0x616a_36e3_c0cf_ef8c;
const LINKED_GROUP: u64 = 0xbea7_9ca4_5e4b_e521;
const STATIC_INDEX: u64 = 0xdd17_1d94_8ccb_47ea;
const SIMULATE: &str = "Unity.Entities.Simulate";
const DISABLED: &str = "Unity.Entities.Disabled";
const INVENTORY_INSTANCE: u64 = 0x7c02_5a52_ca56_3f02;
const BLOOD_CONSUME_SOURCE: u64 = 0x9f76_a48d_02bc_e2bb;
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
    pub placed_mega_statics: usize,
    pub mega_static_template_roots: usize,
    pub mega_static_template_values: usize,
    pub mega_static_template_self_references: usize,
    pub cleared_references: usize,
    pub missing_references: MissingReferences,
    pub admitted_saved_types: usize,
    pub skipped_saved_types: usize,
    pub skipped_saved_components: BTreeMap<String, String>,
    pub admitted_saved_tag_types: usize,
    pub skipped_saved_tag_types: usize,
    pub initialized_simulate_rows: usize,
    pub restored_disabled_rows: usize,
    prefab_root_blobs: Option<std::sync::Arc<PrefabRootBlobs>>,
    pub prefab_blob_bindings: FxHashMap<(Entity, u64, usize), (i32, u64, usize)>,
}

#[derive(Debug, Default)]
pub struct MissingReferences {
    pub nonpersistent: usize,
    pub stale_version: usize,
    pub outside_world_table: usize,
    pub unlisted: usize,
    pub distinct_handles: usize,
    pub by_component: BTreeMap<String, usize>,
}

impl RestoredWorldProjection {
    pub fn prefab_root_blobs(&self) -> Option<&PrefabRootBlobs> {
        self.prefab_root_blobs.as_deref()
    }
    /// Returns a prefab blob binding, including an explicit null source value.
    pub fn prefab_blob(
        &self,
        entity: Entity,
        component_hash: u64,
        element: usize,
    ) -> Result<Option<Option<&PrefabBlobPayload>>> {
        ensure!(self.store.exists(entity)?, "entity does not exist");
        let Some(&(guid, hash, index)) =
            self.prefab_blob_bindings
                .get(&(entity, component_hash, element))
        else {
            return Ok(None);
        };
        let values = self
            .prefab_root_blobs
            .as_ref()
            .context("missing owned prefab blob storage")?
            .values
            .get(&(guid, hash))
            .context("missing bound prefab blob component")?;
        Ok(Some(
            values
                .get(index)
                .context("missing bound prefab blob element")?
                .as_ref(),
        ))
    }

    /// Associates saved component elements with the owned blob values of their source prefab.
    pub fn bind_prefab_root_blobs(
        &mut self,
        snapshot: &SavedSnapshot<'_>,
        blobs: PrefabRootBlobs,
    ) -> Result<()> {
        ensure!(
            self.prefab_root_blobs.is_none()
                || self
                    .types
                    .type_index_of("ProjectM.BloodConsumeSource")
                    .is_none(),
            "cannot replace blob storage while components refer to it"
        );
        let guid_type = snapshot
            .header()
            .persistent_components
            .iter()
            .find(|component| component.stable_type_hash == PREFAB_GUID)
            .context("missing saved prefab GUID type")?
            .saved_type_index;
        let mut bindings = FxHashMap::default();
        for (_, job) in snapshot.jobs() {
            for archetype in &job.archetypes {
                for chunk in &archetype.chunks {
                    let selected: Vec<_> = chunk
                        .columns
                        .iter()
                        .filter(|column| {
                            matches!(
                                column.stable_type_hash,
                                0x9534_7421_34ef_4051
                                    | 0x9f76_a48d_02bc_e2bb
                                    | 0xa413_a8bb_b072_50bc
                            )
                        })
                        .collect();
                    if selected.is_empty() {
                        continue;
                    }
                    let guids = chunk
                        .columns
                        .iter()
                        .find(|column| column.saved_type_index == guid_type)
                        .context("missing prefab GUID column")?;
                    let mut buffer_rows: Vec<_> =
                        selected.iter().map(|column| column.buffers()).collect();
                    for (row, source) in chunk.entities().enumerate() {
                        let guid = i32::from_le_bytes(
                            guids
                                .scalar(row)
                                .context("missing prefab GUID")?
                                .try_into()?,
                        );
                        let target = self
                            .bindings
                            .target(source)
                            .context("missing restored entity")?;
                        for (column, rows) in selected.iter().zip(&mut buffer_rows) {
                            let values = blobs
                                .values
                                .get(&(guid, column.stable_type_hash))
                                .context("missing prefab blob source")?;
                            let count = match rows {
                                Some(rows) => {
                                    rows.next().context("missing buffer row")?.len()
                                        / column.element_size()
                                }
                                None => 1,
                            };
                            ensure!(
                                values.len() == count,
                                "prefab and saved component have different blob element counts"
                            );
                            for element in 0..count {
                                ensure!(
                                    bindings
                                        .insert(
                                            (target, column.stable_type_hash, element),
                                            (guid, column.stable_type_hash, element)
                                        )
                                        .is_none(),
                                    "duplicate prefab blob binding"
                                );
                            }
                        }
                    }
                }
            }
        }
        let blobs = std::sync::Arc::new(blobs);
        self.store.retain_lifetime(blobs.clone());
        self.prefab_root_blobs = Some(blobs);
        self.prefab_blob_bindings = bindings;
        Ok(())
    }
    /// Clones supported components, preserving references to the registered live prefab members.
    pub fn instantiate_prefab(
        &mut self,
        template: &LinkedPrefabTemplate,
        layouts: &TypeLayouts,
    ) -> Result<Vec<Entity>> {
        let by_guid: BTreeMap<_, _> = self
            .prefabs
            .iter()
            .map(|prefab| (prefab.prefab_guid, prefab))
            .collect();
        template.instantiate_with_external(&mut self.store, &mut self.types, layouts, |key| {
            key.resolve(by_guid.get(&key.guid)?)
        })
    }

    /// Allocates real ECS handles for the prefab topology and every saved job row,
    /// then restores the validated scalar and buffer subset.
    pub fn build(
        snapshot: &SavedSnapshot<'_>,
        layouts: &TypeLayouts,
        catalogs: &[ScenePrefabCatalog],
    ) -> Result<Self> {
        Self::build_inner(snapshot, layouts, catalogs, None, None)
    }

    /// Includes MegaStatic identity and world transform in the projection before remapping saved references.
    pub fn build_with_terrain(
        snapshot: &SavedSnapshot<'_>,
        layouts: &TypeLayouts,
        catalogs: &[ScenePrefabCatalog],
        plan: &MegaStaticRestorePlan,
        terrain: &StaticTransformCatalog,
    ) -> Result<Self> {
        Self::build_inner(snapshot, layouts, catalogs, Some((plan, terrain)), None)
    }

    /// Restores supported components using blob data owned by the resulting projection.
    pub fn build_with_terrain_and_blobs(
        snapshot: &SavedSnapshot<'_>,
        layouts: &TypeLayouts,
        catalogs: &[ScenePrefabCatalog],
        plan: &MegaStaticRestorePlan,
        terrain: &StaticTransformCatalog,
        blobs: PrefabRootBlobs,
    ) -> Result<Self> {
        let mut restored = Self::build_inner(
            snapshot,
            layouts,
            catalogs,
            Some((plan, terrain)),
            Some(&blobs),
        )?;
        restored.bind_prefab_root_blobs(snapshot, blobs)?;
        Ok(restored)
    }

    fn build_inner(
        snapshot: &SavedSnapshot<'_>,
        layouts: &TypeLayouts,
        catalogs: &[ScenePrefabCatalog],
        placement: Option<(&MegaStaticRestorePlan, &StaticTransformCatalog)>,
        root_blobs: Option<&PrefabRootBlobs>,
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
        let placement_types = if placement.is_some() {
            Some((
                register(STATIC_INDEX)?,
                register(TRANSLATION_HASH)?,
                register(ROTATION_HASH)?,
            ))
        } else {
            None
        };
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
                (register(hash)?, stride, offsets.to_vec()),
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
            buffers.insert(component.saved_type_index, (ty, stride, offsets.to_vec()));
        }
        let linked_group = linked_group.context("missing live linked group type")?;
        let observed: BTreeSet<_> = snapshot
            .jobs()
            .iter()
            .flat_map(|(_, job)| &job.archetypes)
            .flat_map(|archetype| &archetype.chunks)
            .flat_map(|chunk| &chunk.columns)
            .map(|column| column.saved_type_index)
            .collect();
        let blob_types: BTreeSet<_> = snapshot
            .header()
            .blobs
            .iter()
            .map(|blob| blob.component_stable_type_hash)
            .collect();
        let mut registered = FxHashMap::from_iter([
            (ENTITY_HASH, entity_type),
            (PREFAB_TAG, prefab_tag),
            (PREFAB_GUID, prefab_guid),
        ]);
        // Tags are carried by archetype declarations and have no per-row value column.
        let observed_tags: BTreeSet<_> = snapshot
            .jobs()
            .iter()
            .flat_map(|(_, job)| &job.archetypes)
            .flat_map(|archetype| archetype.types.iter().copied())
            .filter(|ty| ty.is_zero_sized())
            .collect();
        let allowed_tag_flags = TypeIndex::K_ZERO_SIZED_MASK
            | TypeIndex::K_HAS_ENTITY_REFERENCES_MASK
            | TypeIndex::K_ENABLEABLE_MASK
            | TypeIndex::K_IEQUATABLE_MASK;
        let mut tag_layouts = FxHashMap::default();
        for layout in layouts.recovered_types() {
            if layout.spec.category != TypeCategory::ComponentData
                || layout.spec.size_in_chunk != 0
                || !layout.entity_offsets.is_empty()
                || layout.spec.flags & !allowed_tag_flags != 0
            {
                continue;
            }
            ensure!(
                tag_layouts.insert(layout.name.as_str(), layout).is_none(),
                "ambiguous server tag layout for {}",
                layout.name
            );
        }
        let mut tags = FxHashMap::default();
        let mut simulate = None;
        let mut disabled = None;
        for component in &snapshot.header().required_components {
            let saved_type = component.saved_type_index;
            let name = component.full_name.as_str();
            if !observed_tags.contains(&saved_type)
                || (saved_type.is_enableable() && name != SIMULATE)
            {
                continue;
            }
            let Some(layout) = tag_layouts.get(name) else {
                continue;
            };
            if saved_type.flags() != layout.spec.flags {
                continue;
            }
            let live_type = match registered.entry(layout.stable_hash) {
                std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    *entry.insert(register(layout.stable_hash)?)
                }
            };
            tags.insert(saved_type, live_type);
            match name {
                SIMULATE => simulate = Some(saved_type),
                DISABLED => disabled = Some((saved_type, live_type)),
                _ => {}
            }
        }
        if let Some((index, translation, rotation)) = placement_types {
            registered.extend([
                (STATIC_INDEX, index),
                (TRANSLATION_HASH, translation),
                (ROTATION_HASH, rotation),
            ]);
        }
        for (saved_type, (live_type, _, _)) in scalars.iter().chain(&buffers) {
            let hash = snapshot
                .header()
                .persistent_components
                .iter()
                .find(|component| component.saved_type_index == *saved_type)
                .context("missing admitted saved component")?
                .stable_type_hash;
            registered.insert(hash, *live_type);
        }
        for component in &snapshot.header().persistent_components {
            let saved_type = component.saved_type_index;
            let hash = component.stable_type_hash;
            if !observed.contains(&saved_type)
                || scalars.contains_key(&saved_type)
                || buffers.contains_key(&saved_type)
                || blob_types.contains(&hash)
            {
                continue;
            }
            let Some(layout) = layouts.get(hash) else {
                continue;
            };
            let category = layout.spec.category;
            let buffer = category == TypeCategory::BufferData;
            let excluded = TypeIndex::K_MANAGED_MASK
                | TypeIndex::K_CLEANUP_MASK
                | TypeIndex::K_ENABLEABLE_MASK
                | TypeIndex::K_CHUNK_COMPONENT_MASK;
            let stride = if buffer {
                layout.spec.element_size
            } else {
                layout.spec.size_in_chunk
            };
            // StartItems is marked SetToDefault by the original persistence loader.
            let inventory_instance = hash == INVENTORY_INSTANCE
                && layout.name == "ProjectM.InventoryInstanceElement"
                && buffer
                && stride == 48
                && layout.entity_offsets == [16]
                && layout.blob_offsets.as_deref() == Some(&[40]);
            let blood_source = root_blobs.is_some()
                && hash == BLOOD_CONSUME_SOURCE
                && layout.name == "ProjectM.BloodConsumeSource"
                && !buffer
                && stride == 40
                && layout.entity_offsets.is_empty()
                && layout.blob_offsets.as_deref() == Some(&[0]);
            if (!layout.portable_value && !inventory_instance && !blood_source)
                || stride <= 0
                || stride != component.component_size
                || buffer != saved_type.is_buffer()
                || saved_type.has_entity_references() == layout.entity_offsets.is_empty()
                || (layout.spec.flags | saved_type.value as u32) & excluded != 0
                || !matches!(
                    category,
                    TypeCategory::BufferData | TypeCategory::ComponentData
                )
            {
                continue;
            }
            let mut offsets: Vec<_> = layout
                .entity_offsets
                .iter()
                .map(|&offset| offset as usize)
                .collect();
            offsets.sort_unstable();
            if offsets.iter().any(|&offset| offset + 8 > stride as usize)
                || offsets.windows(2).any(|pair| pair[0] + 8 > pair[1])
            {
                continue;
            }
            let live_type = match registered.entry(hash) {
                std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
                std::collections::hash_map::Entry::Vacant(entry) => *entry.insert(register(hash)?),
            };
            let target = if buffer { &mut buffers } else { &mut scalars };
            target.insert(saved_type, (live_type, stride as usize, offsets));
        }
        let admitted_saved_types = observed
            .iter()
            .filter(|ty| scalars.contains_key(ty) || buffers.contains_key(ty))
            .count();
        let skipped_saved_types = observed.len() - admitted_saved_types;
        let mut skipped_saved_components = BTreeMap::new();
        for saved_type in observed
            .iter()
            .filter(|ty| !scalars.contains_key(ty) && !buffers.contains_key(ty))
        {
            let component = snapshot
                .header()
                .persistent_components
                .iter()
                .find(|component| component.saved_type_index == *saved_type)
                .context("missing skipped saved component metadata")?;
            let name = snapshot
                .header()
                .required_components
                .iter()
                .find(|required| required.saved_type_index == *saved_type)
                .context("missing skipped saved component name")?;
            let reason = if blob_types.contains(&component.stable_type_hash) {
                "blob references require remapping"
            } else if layouts.get(component.stable_type_hash).is_none() {
                "server layout unavailable"
            } else {
                "layout or component flags not supported"
            };
            ensure!(
                skipped_saved_components
                    .insert(name.full_name.clone(), reason.to_owned())
                    .is_none(),
                "duplicate skipped saved component name {}",
                name.full_name
            );
        }
        let mut instance_templates = FxHashMap::default();
        if placement.is_some() {
            for catalog in catalogs {
                for (&guid, values) in &catalog.instance_templates {
                    let mut registered_values = Vec::new();
                    for value in values {
                        if blob_types.contains(&value.stable_hash) {
                            continue;
                        }
                        let live_type = match registered.entry(value.stable_hash) {
                            std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
                            std::collections::hash_map::Entry::Vacant(entry) => {
                                *entry.insert(register(value.stable_hash)?)
                            }
                        };
                        registered_values.push((live_type, value));
                    }
                    ensure!(
                        instance_templates.insert(guid, registered_values).is_none(),
                        "duplicate MegaStatic template"
                    );
                }
            }
        }
        let mut store = EntityComponentStore::new(
            i32::try_from(snapshot.entity_count() + 2)?,
            SpecialTypes {
                entity_type,
                prefab: prefab_tag,
                disabled: disabled.map_or(SpecialTypes::UNSET, |(_, live)| live),
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
        let mut initialized_simulate_rows = 0;
        let mut restored_disabled_rows = 0;
        for (_, job) in snapshot.jobs() {
            for archetype in &job.archetypes {
                let projected_tags: Vec<_> = archetype
                    .types
                    .iter()
                    .filter_map(|ty| tags.get(ty).copied())
                    .collect();
                let has_simulate = simulate.is_some_and(|ty| archetype.types.contains(&ty));
                let is_disabled = disabled.is_some_and(|(ty, _)| archetype.types.contains(&ty));
                for chunk in &archetype.chunks {
                    let mut projected: Vec<_> = chunk
                        .columns
                        .iter()
                        .filter_map(|column| {
                            scalars
                                .get(&column.saved_type_index)
                                .or_else(|| buffers.get(&column.saved_type_index))
                                .map(|layout| layout.0)
                        })
                        .collect();
                    projected.extend(&projected_tags);
                    let row_count = chunk.entities().len();
                    if has_simulate {
                        initialized_simulate_rows += row_count;
                    }
                    if is_disabled {
                        restored_disabled_rows += row_count;
                    }
                    let arch = store.get_or_create_archetype(&types, &projected)?;
                    fresh.extend(store.create_entities(arch, row_count)?);
                }
            }
        }
        let mut bindings = snapshot.bind_entities(fresh, &prefabs)?;
        ensure!(
            bindings.issues().is_empty(),
            "incomplete prefab/job bindings: {:?}",
            bindings.issues().first()
        );
        let mut placed_mega_statics = 0;
        let mut mega_static_template_values = 0;
        let mut mega_static_template_self_references = 0;
        if let (Some((plan, terrain)), Some((index, translation, rotation))) =
            (placement, placement_types)
        {
            let fallback_arch = store
                .get_or_create_archetype(&types, &[index, prefab_guid, translation, rotation])?;
            let mut template_archetypes = FxHashMap::default();
            for (&guid, values) in &instance_templates {
                let mut components = vec![index, prefab_guid, translation, rotation];
                components.extend(values.iter().map(|(ty, _)| *ty));
                template_archetypes
                    .insert(guid, store.get_or_create_archetype(&types, &components)?);
            }
            let mut patched = Vec::new();
            for entry in &plan.entries {
                let arch = template_archetypes
                    .get(&entry.prefab_guid)
                    .copied()
                    .unwrap_or(fallback_arch);
                let instance = store.create_entity(arch)?;
                ensure!(
                    bindings.target(entry.saved_prefab_root).is_some()
                        && bindings.target(entry.saved_manager).is_some(),
                    "unbound MegaStatic prefab or manager"
                );
                let transform = terrain
                    .placed(entry.terrain_chunk, entry.transform_index)
                    .context("missing baked transform for MegaStatic instance")?;
                let packed_index =
                    u32::from(entry.terrain_chunk) | (u32::from(entry.transform_index) << 16);
                store.set_component_bytes(instance, index, &packed_index.to_le_bytes())?;
                store.set_component_bytes(
                    instance,
                    prefab_guid,
                    &entry.prefab_guid.to_le_bytes(),
                )?;
                if let Some(values) = instance_templates.get(&entry.prefab_guid) {
                    for (ty, value) in values {
                        let bytes = value.patch(&[instance], &mut patched)?;
                        mega_static_template_self_references += value.reference_patches.len();
                        if value.is_buffer {
                            store.set_buffer_payload(&types, instance, *ty, bytes)?;
                        } else {
                            store.set_component_bytes(instance, *ty, bytes)?;
                        }
                        mega_static_template_values += 1;
                    }
                }
                store.set_component_bytes(
                    instance,
                    translation,
                    transform.position.map(f32::to_le_bytes).as_flattened(),
                )?;
                store.set_component_bytes(
                    instance,
                    rotation,
                    transform.rotation.map(f32::to_le_bytes).as_flattened(),
                )?;
                bindings.bind_recreated(entry.saved_instance, instance)?;
                placed_mega_statics += 1;
            }
        }
        let mut copied_scalar_values = 0;
        let mut copied_buffer_rows = 0;
        let mut copied_buffer_elements = 0;
        let mut cleared_references = 0;
        let mut missing_references = MissingReferences::default();
        let mut missed_handles = rustc_hash::FxHashSet::default();
        let world = snapshot.world_data();
        let mut nonpersistent = vec![false; world.entity_capacity()];
        if let Some(archetypes) = &world.nonpersistent_archetypes {
            for index in archetypes
                .iter()
                .flat_map(|archetype| &archetype.chunks)
                .flat_map(|chunk| chunk.entity_indices())
            {
                nonpersistent[index] = true;
            }
        }
        let mut missed_totals = FxHashMap::default();
        for (_, job) in snapshot.jobs() {
            for archetype in &job.archetypes {
                for chunk in &archetype.chunks {
                    for column in &chunk.columns {
                        let mut column_misses = 0;
                        let mut record_miss = |source: Entity| {
                            column_misses += 1;
                            missed_handles.insert(source);
                            let Ok(index) = usize::try_from(source.index) else {
                                missing_references.outside_world_table += 1;
                                return;
                            };
                            match world.entity_version(index) {
                                None => missing_references.outside_world_table += 1,
                                Some(version) if version != source.version => {
                                    missing_references.stale_version += 1
                                }
                                Some(_) if nonpersistent[index] => {
                                    missing_references.nonpersistent += 1
                                }
                                Some(_) => missing_references.unlisted += 1,
                            }
                        };
                        if let Some((live_type, stride, offsets)) =
                            buffers.get(&column.saved_type_index)
                        {
                            let payloads =
                                column.buffers().context("expected saved buffer rows")?;
                            let mut data = Vec::new();
                            for (source, payload) in chunk.entities().zip(payloads) {
                                let payload = if offsets.is_empty()
                                    && column.stable_type_hash != INVENTORY_INSTANCE
                                {
                                    payload
                                } else {
                                    data.clear();
                                    data.extend_from_slice(payload);
                                    if column.stable_type_hash == INVENTORY_INSTANCE {
                                        for element in data.chunks_exact_mut(*stride) {
                                            element[40..48].fill(0);
                                        }
                                    }
                                    bindings.patch_references_with(
                                        &mut data,
                                        *stride,
                                        offsets,
                                        &mut record_miss,
                                    )?;
                                    &data
                                };
                                store.set_buffer_payload(
                                    &types,
                                    bindings.target(source).context("missing live entity")?,
                                    *live_type,
                                    payload,
                                )?;
                                copied_buffer_rows += 1;
                                copied_buffer_elements += payload.len() / *stride;
                            }
                            if column_misses > 0 {
                                *missed_totals.entry(*live_type).or_insert(0) += column_misses;
                                cleared_references += column_misses;
                            }
                            continue;
                        }
                        let Some((live_type, stride, offsets)) =
                            scalars.get(&column.saved_type_index)
                        else {
                            continue;
                        };
                        let mut data = vec![0; *stride];
                        for (row, source) in chunk.entities().enumerate() {
                            let payload = column.scalar(row).context("missing saved scalar row")?;
                            let payload = if offsets.is_empty()
                                && column.stable_type_hash != BLOOD_CONSUME_SOURCE
                            {
                                payload
                            } else {
                                data.copy_from_slice(payload);
                                if column.stable_type_hash == BLOOD_CONSUME_SOURCE {
                                    let guid_column = chunk
                                        .columns
                                        .iter()
                                        .find(|saved| saved.stable_type_hash == PREFAB_GUID)
                                        .context("missing blood source prefab GUID")?;
                                    let guid = i32::from_le_bytes(
                                        guid_column
                                            .scalar(row)
                                            .context("missing blood source prefab GUID value")?
                                            .try_into()?,
                                    );
                                    let value = root_blobs
                                        .context("missing prefab blob sources")?
                                        .values
                                        .get(&(guid, BLOOD_CONSUME_SOURCE))
                                        .context("missing blood source prefab blob")?;
                                    ensure!(
                                        value.len() == 1,
                                        "invalid blood source prefab blob count"
                                    );
                                    let address =
                                        value[0].as_ref().map_or(0, |blob| blob.payload_address());
                                    data[..8].copy_from_slice(&address.to_le_bytes());
                                }
                                bindings.patch_references_with(
                                    &mut data,
                                    *stride,
                                    offsets,
                                    &mut record_miss,
                                )?;
                                &data
                            };
                            store.set_component_bytes(
                                bindings.target(source).context("missing live entity")?,
                                *live_type,
                                payload,
                            )?;
                            copied_scalar_values += 1;
                        }
                        if column_misses > 0 {
                            *missed_totals.entry(*live_type).or_insert(0) += column_misses;
                            cleared_references += column_misses;
                        }
                    }
                }
            }
        }
        missing_references.distinct_handles = missed_handles.len();
        for (ty, count) in missed_totals {
            let name = &types
                .type_info(ty)
                .context("missing reference component layout")?
                .name;
            missing_references.by_component.insert(name.clone(), count);
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
            placed_mega_statics,
            mega_static_template_roots: instance_templates.len(),
            mega_static_template_values,
            mega_static_template_self_references,
            cleared_references,
            missing_references,
            admitted_saved_types,
            skipped_saved_types,
            skipped_saved_components,
            admitted_saved_tag_types: tags.len(),
            skipped_saved_tag_types: observed_tags.len() - tags.len(),
            initialized_simulate_rows,
            restored_disabled_rows,
            prefab_root_blobs: None,
            prefab_blob_bindings: FxHashMap::default(),
        })
    }
}
