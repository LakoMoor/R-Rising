use std::collections::{BTreeMap, BTreeSet};

use anyhow::{ensure, Context, Result};
use rrising_ecs::{Entity, EntityComponentStore, TypeCategory, TypeIndex, TypeManager};

use crate::entity_scenes::{SceneBufferData, SceneFile};
use crate::persistence::PrefabReference;
use crate::type_layouts::{
    EnabledSceneRows, ResolvedScene, TypeLayouts, ENTITY_HASH, LOCAL_TO_WORLD_HASH,
    LOCAL_TRANSFORM_HASH, ROTATION_HASH, TRANSLATION_HASH,
};

pub const REGISTER_PREFAB_HASH: u64 = 0xa965_c72e_229a_11cf;
pub const REGISTER_PREFAB_EVENT_HASH: u64 = 0x340f_ac1f_a9e0_4679;
pub const LINKED_ENTITY_GROUP_HASH: u64 = 0xbea7_9ca4_5e4b_e521;
const STATIC_INDEX_HASH: u64 = 0xdd17_1d94_8ccb_47ea;
const PREFAB_GUID_HASH: u64 = 0xb181_3197_8820_0ae1;
const PREFAB_TAG_HASH: u64 = 0x616a_36e3_c0cf_ef8c;
const REGISTER_COLLECTION_HASH: u64 = 0x7985_6652_5bcc_c843;

pub const SERVER_PREFAB_SCENES: [(&str, usize); 9] = [
    ("ad37c0edd53365b40acb48df81e07046", 2023),
    ("8137c0edd53365b40acb48df81e07046", 1189),
    ("153b723300728974d8e2bad70c3fd1f2", 3687),
    ("3429e7b0ff91dfa4db4318bcdb0e63bc", 1558),
    ("4361b3a547128724892dae4688ec7146", 395),
    ("b2f35358da120c04c92b9256f40d76ce", 4842),
    ("295c92b7a799a3443b40e9ef192b5727", 8548),
    ("5e3be03ad2064bc4a9644c4a1a30af42", 364),
    ("de0ffb819455f8f4c8eea0748b1012b6", 977),
];

#[derive(Debug, Default)]
pub struct ScenePrefabCatalog {
    pub prefabs: Vec<PrefabReference>,
    /// Candidate root rows whose component layout could not be reconstructed.
    pub unresolved_rows: usize,
    pub instance_templates: BTreeMap<i32, Vec<PrefabTemplateValue>>,
    pub linked_templates: BTreeMap<i32, LinkedPrefabTemplate>,
}

#[derive(Debug)]
pub struct PrefabTemplateValue {
    pub stable_hash: u64,
    pub is_buffer: bool,
    pub enabled: Option<bool>,
    pub bytes: Vec<u8>,
    pub reference_patches: Vec<(usize, usize)>,
    pub external_patches: Vec<(usize, PrefabMemberKey)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PrefabMemberKey {
    pub guid: i32,
    pub ordinal: usize,
}

impl PrefabMemberKey {
    pub fn resolve(self, prefab: &PrefabReference) -> Option<Entity> {
        if prefab.prefab_guid != self.guid {
            return None;
        }
        if self.ordinal == 0 {
            Some(prefab.entity)
        } else {
            prefab.linked_entities.get(self.ordinal).copied()
        }
    }
}

#[derive(Debug)]
pub struct LinkedPrefabTemplate {
    pub members: Vec<Vec<PrefabTemplateValue>>,
    pub has_group: bool,
}

impl LinkedPrefabTemplate {
    /// Clones the admitted values into fresh ECS rows and points group-local references
    /// at the corresponding fresh row. The returned handles follow LinkedEntityGroup order.
    pub fn instantiate(
        &self,
        store: &mut EntityComponentStore,
        types: &mut TypeManager,
        layouts: &TypeLayouts,
    ) -> Result<Vec<Entity>> {
        self.instantiate_with_external(store, types, layouts, |_| None)
    }

    /// Resolves cross-group references against existing live prefab members. When any external
    /// field of a component cannot be resolved, that component is not installed in the clone.
    pub fn instantiate_with_external(
        &self,
        store: &mut EntityComponentStore,
        types: &mut TypeManager,
        layouts: &TypeLayouts,
        mut resolve: impl FnMut(PrefabMemberKey) -> Option<Entity>,
    ) -> Result<Vec<Entity>> {
        ensure!(!self.members.is_empty(), "prefab template has no root");
        let mut registry = rustc_hash::FxHashMap::default();
        for info in types.infos() {
            registry.insert(info.stable_type_hash, info.type_index);
        }
        let mut register = |hash| -> Result<TypeIndex> {
            match registry.entry(hash) {
                std::collections::hash_map::Entry::Occupied(entry) => Ok(*entry.get()),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    let layout = layouts.get(hash).context("missing linked prefab layout")?;
                    Ok(*entry.insert(types.register(layout.spec.clone())?))
                }
            }
        };
        let group = self
            .has_group
            .then(|| register(LINKED_ENTITY_GROUP_HASH))
            .transpose()?;
        let typed: Vec<Vec<_>> = self
            .members
            .iter()
            .map(|values| {
                values
                    .iter()
                    .map(|value| {
                        let targets: Option<Vec<_>> = value
                            .external_patches
                            .iter()
                            .map(|&(offset, key)| resolve(key).map(|entity| (offset, entity)))
                            .collect();
                        let Some(targets) = targets else {
                            return Ok(None);
                        };
                        for &(_, entity) in &targets {
                            ensure!(
                                entity != Entity::NULL && store.exists(entity)?,
                                "external prefab binding is not live"
                            );
                        }
                        Ok(Some((register(value.stable_hash)?, targets)))
                    })
                    .collect::<Result<_>>()
            })
            .collect::<Result<_>>()?;
        let mut instances = Vec::with_capacity(typed.len());
        for (ordinal, member_types) in typed.iter().enumerate() {
            let mut set: Vec<_> = member_types.iter().flatten().map(|(ty, _)| *ty).collect();
            if ordinal == 0 {
                set.extend(group);
            }
            let arch = store.get_or_create_archetype(types, &set)?;
            instances.push(store.create_entity(arch)?);
        }
        let mut scratch = Vec::new();
        for ((values, typed), &entity) in self.members.iter().zip(&typed).zip(&instances) {
            for (value, field) in values.iter().zip(typed) {
                let Some((ty, targets)) = field else { continue };
                let payload = value.patch_with_external(&instances, targets, &mut scratch)?;
                if value.is_buffer {
                    store.set_buffer_payload(types, entity, *ty, payload)?;
                } else {
                    store.set_component_bytes(entity, *ty, payload)?;
                }
                if let Some(enabled) = value.enabled {
                    store.set_component_enabled(entity, *ty, enabled)?;
                }
            }
        }
        if let Some(group) = group {
            let packed: Vec<u8> = instances
                .iter()
                .flat_map(|entity| {
                    entity
                        .index
                        .to_le_bytes()
                        .into_iter()
                        .chain(entity.version.to_le_bytes())
                })
                .collect();
            store.set_buffer_payload(types, instances[0], group, &packed)?;
        }
        Ok(instances)
    }
}

impl PrefabTemplateValue {
    pub(crate) fn patch<'a>(
        &'a self,
        instances: &[Entity],
        scratch: &'a mut Vec<u8>,
    ) -> Result<&'a [u8]> {
        self.patch_with_external(instances, &[], scratch)
    }

    fn patch_with_external<'a>(
        &'a self,
        instances: &[Entity],
        targets: &[(usize, Entity)],
        scratch: &'a mut Vec<u8>,
    ) -> Result<&'a [u8]> {
        ensure!(
            targets.len() == self.external_patches.len(),
            "incomplete external prefab patch"
        );
        if self.reference_patches.is_empty() && targets.is_empty() {
            return Ok(&self.bytes);
        }
        scratch.clear();
        scratch.extend_from_slice(&self.bytes);
        for &(offset, ordinal) in &self.reference_patches {
            let entity = instances
                .get(ordinal)
                .context("invalid prefab reference ordinal")?;
            patch_entity_at(scratch, offset, *entity)?;
        }
        for &(offset, entity) in targets {
            patch_entity_at(scratch, offset, entity)?;
        }
        Ok(scratch)
    }
}

fn patch_entity_at(bytes: &mut [u8], offset: usize, entity: Entity) -> Result<()> {
    let target = bytes
        .get_mut(offset..)
        .and_then(|tail| tail.get_mut(..8))
        .context("invalid prefab reference offset")?;
    target[..4].copy_from_slice(&entity.index.to_le_bytes());
    target[4..].copy_from_slice(&entity.version.to_le_bytes());
    Ok(())
}

impl ScenePrefabCatalog {
    pub fn read_server_linked_prefab(
        data_dir: &std::path::Path,
        layouts: &TypeLayouts,
        guid: i32,
    ) -> Result<LinkedPrefabTemplate> {
        let directory = data_dir.join("StreamingAssets/EntityScenes");
        let requested = BTreeSet::from([guid]);
        let mut template = None;
        for (scene_id, _) in SERVER_PREFAB_SCENES {
            let scene = SceneFile::open(&directory.join(format!("{scene_id}.0.entities")))?;
            let mut catalog = Self::read_linked_templates(&scene, layouts, &requested)?;
            if let Some(found) = catalog.linked_templates.remove(&guid) {
                ensure!(template.is_none(), "duplicate server prefab GUID {guid}");
                template = Some(found);
            }
        }
        template.with_context(|| format!("unknown server prefab GUID {guid}"))
    }

    pub fn read_server_game_data(
        data_dir: &std::path::Path,
        layouts: &TypeLayouts,
    ) -> Result<Vec<Self>> {
        Self::read_server_game_data_for(data_dir, layouts, &BTreeSet::new())
    }

    /// Extracts additional component values for selected MegaStatic prefab roots.
    pub fn read_server_game_data_for(
        data_dir: &std::path::Path,
        layouts: &TypeLayouts,
        requested: &BTreeSet<i32>,
    ) -> Result<Vec<Self>> {
        let directory = data_dir.join("StreamingAssets/EntityScenes");
        SERVER_PREFAB_SCENES
            .iter()
            .map(|(scene_id, expected)| {
                let scene = SceneFile::open(&directory.join(format!("{scene_id}.0.entities")))?;
                let catalog = Self::read_with_templates(&scene, layouts, requested, false)
                    .with_context(|| format!("reading {scene_id} prefab registrations"))?;
                ensure!(
                    catalog.unresolved_rows == 0 && catalog.prefabs.len() == *expected,
                    "unexpected prefab registration counts in {scene_id}"
                );
                Ok(catalog)
            })
            .collect()
    }

    /// Extracts scene-local handles. Different scenes must be remapped before joining catalogs.
    pub fn read(scene: &SceneFile, layouts: &TypeLayouts) -> Result<Self> {
        Self::read_with_templates(scene, layouts, &BTreeSet::new(), false)
    }

    /// Reads cloneable values of selected roots and all of their linked members.
    pub fn read_linked_templates(
        scene: &SceneFile,
        layouts: &TypeLayouts,
        requested: &BTreeSet<i32>,
    ) -> Result<Self> {
        Self::read_with_templates(scene, layouts, requested, true)
    }

    fn read_with_templates(
        scene: &SceneFile,
        layouts: &TypeLayouts,
        requested: &BTreeSet<i32>,
        linked: bool,
    ) -> Result<Self> {
        let resolved = layouts.resolve_scene(scene)?;
        let buffers = scene.buffer_data()?;
        let mut result = Self::default();
        let mut locations = rustc_hash::FxHashMap::default();
        for (chunk_index, chunk) in resolved.chunks.iter().enumerate() {
            let arch_index = usize::try_from(chunk.header.archetype_index)?;
            let source = &resolved.source.archetypes[arch_index];
            let registers = source.types.iter().any(|slot| {
                !slot.chunk_component
                    && resolved.source.unique_type_hashes[usize::from(slot.unique_index)]
                        == REGISTER_PREFAB_HASH
            });
            let count = usize::try_from(chunk.header.count)?;
            if resolved.archetypes[arch_index].is_none() {
                if registers {
                    result.unresolved_rows += count;
                }
                continue;
            }
            for row in 0..count {
                let entity = read_entity(
                    resolved
                        .component_bytes(chunk_index, row, ENTITY_HASH)?
                        .context("missing scene entity bytes")?,
                )?;
                ensure!(
                    locations.insert(entity, (chunk_index, row)).is_none(),
                    "duplicate scene entity"
                );
                if registers {
                    let guid = resolved
                        .component_bytes(chunk_index, row, REGISTER_PREFAB_EVENT_HASH)?
                        .context("missing register-prefab event")?;
                    let record = resolved
                        .component_bytes(chunk_index, row, REGISTER_PREFAB_HASH)?
                        .context("missing register-prefab record")?;
                    ensure!(
                        record.len() == 664,
                        "unsupported register-prefab record size"
                    );
                    let root = read_entity(&record[656..])?;
                    result.prefabs.push(PrefabReference {
                        prefab_guid: i32::from_le_bytes(guid.try_into()?),
                        entity: root,
                        linked_entities: Vec::new(),
                    });
                }
            }
        }
        let mut missing_roots = 0;
        for prefab in &mut result.prefabs {
            let Some(&(chunk, row)) = locations.get(&prefab.entity) else {
                missing_roots += 1;
                continue;
            };
            let linked_bytes =
                resolved.buffer_payload(&buffers, chunk, row, LINKED_ENTITY_GROUP_HASH, 8)?;
            prefab.linked_entities = linked_bytes
                .unwrap_or_default()
                .as_chunks::<8>()
                .0
                .iter()
                .map(|bytes| read_entity(bytes))
                .collect::<Result<Vec<_>>>()?;
            ensure!(
                prefab
                    .linked_entities
                    .first()
                    .is_none_or(|&root| root == prefab.entity),
                "prefab group does not start with its root"
            );
            ensure!(
                prefab
                    .linked_entities
                    .iter()
                    .all(|entity| locations.contains_key(entity)),
                "prefab group contains a handle outside the reconstructed scene"
            );
        }
        result
            .prefabs
            .retain(|prefab| locations.contains_key(&prefab.entity));
        result.unresolved_rows += missing_roots;
        let enabled = if linked {
            Some(EnabledSceneRows::read(scene, &resolved)?)
        } else {
            None
        };
        let mut external = rustc_hash::FxHashMap::default();
        if linked {
            let mut ambiguous = rustc_hash::FxHashSet::default();
            for prefab in &result.prefabs {
                let handles = if prefab.linked_entities.is_empty() {
                    std::slice::from_ref(&prefab.entity)
                } else {
                    &prefab.linked_entities
                };
                for (ordinal, &entity) in handles.iter().enumerate() {
                    let key = PrefabMemberKey {
                        guid: prefab.prefab_guid,
                        ordinal,
                    };
                    if external
                        .insert(entity, key)
                        .is_some_and(|previous| previous != key)
                    {
                        ambiguous.insert(entity);
                    }
                }
            }
            external.retain(|entity, _| !ambiguous.contains(entity));
        }
        for prefab in &result.prefabs {
            let (chunk, row) = locations[&prefab.entity];
            if !requested.contains(&prefab.prefab_guid) {
                continue;
            }
            if linked {
                let members = if prefab.linked_entities.is_empty() {
                    std::slice::from_ref(&prefab.entity)
                } else {
                    &prefab.linked_entities
                };
                let ordinals: rustc_hash::FxHashMap<_, _> = members
                    .iter()
                    .enumerate()
                    .map(|(ordinal, &entity)| (entity, ordinal))
                    .collect();
                ensure!(
                    ordinals.len() == members.len(),
                    "duplicate linked prefab member"
                );
                let values = members
                    .iter()
                    .map(|entity| {
                        let &(chunk, row) = locations
                            .get(entity)
                            .context("missing linked prefab member")?;
                        extract_values(
                            &resolved,
                            &buffers,
                            layouts,
                            (chunk, row),
                            &ordinals,
                            Some(&external),
                            enabled.as_ref(),
                        )
                    })
                    .collect::<Result<_>>()?;
                ensure!(
                    result
                        .linked_templates
                        .insert(
                            prefab.prefab_guid,
                            LinkedPrefabTemplate {
                                members: values,
                                has_group: !prefab.linked_entities.is_empty(),
                            }
                        )
                        .is_none(),
                    "duplicate linked prefab template"
                );
            } else {
                let ordinals = rustc_hash::FxHashMap::from_iter([(prefab.entity, 0)]);
                let values = extract_values(
                    &resolved,
                    &buffers,
                    layouts,
                    (chunk, row),
                    &ordinals,
                    None,
                    None,
                )?;
                ensure!(
                    result
                        .instance_templates
                        .insert(prefab.prefab_guid, values)
                        .is_none(),
                    "duplicate prefab template GUID"
                );
            }
        }
        Ok(result)
    }
}

fn extract_values(
    resolved: &ResolvedScene<'_>,
    buffers: &SceneBufferData<'_>,
    layouts: &TypeLayouts,
    (chunk, row): (usize, usize),
    ordinals: &rustc_hash::FxHashMap<Entity, usize>,
    external: Option<&rustc_hash::FxHashMap<Entity, PrefabMemberKey>>,
    enabled: Option<&EnabledSceneRows>,
) -> Result<Vec<PrefabTemplateValue>> {
    let arch = resolved.archetypes[resolved.chunks[chunk].header.archetype_index as usize]
        .as_ref()
        .context("unresolved prefab archetype")?;
    let mut template = Vec::new();
    for (slot, (&stable_hash, &ty)) in arch.hashes.iter().zip(&arch.types).enumerate() {
        if [
            ENTITY_HASH,
            PREFAB_TAG_HASH,
            LINKED_ENTITY_GROUP_HASH,
            REGISTER_PREFAB_HASH,
            REGISTER_PREFAB_EVENT_HASH,
            REGISTER_COLLECTION_HASH,
        ]
        .contains(&stable_hash)
            || (external.is_none()
                && [
                    PREFAB_GUID_HASH,
                    STATIC_INDEX_HASH,
                    TRANSLATION_HASH,
                    ROTATION_HASH,
                    LOCAL_TO_WORLD_HASH,
                    LOCAL_TRANSFORM_HASH,
                ]
                .contains(&stable_hash))
        {
            continue;
        }
        let layout = layouts
            .get(stable_hash)
            .context("missing prefab component layout")?;
        if !layout.portable_value
            || layout.name.starts_with("ProjectM.Network.")
            || ty.is_chunk_component()
            || (ty.is_zero_sized() && external.is_none())
            || (ty.is_enableable() && enabled.is_none())
            || ty.is_cleanup_component()
            || !matches!(
                layout.spec.category,
                TypeCategory::ComponentData | TypeCategory::BufferData
            )
            || ty.value as u32 & (TypeIndex::K_MANAGED_MASK | TypeIndex::K_CLEANUP_MASK) != 0
        {
            continue;
        }
        let is_enabled = if ty.is_enableable() {
            let memory_slot = usize::try_from(arch.data.layout.archetype_to_memory_order[slot])?;
            Some(enabled.context("missing prefab enable data")?.get(
                chunk,
                row,
                memory_slot,
                resolved.chunks[chunk].header.count,
            )?)
        } else {
            None
        };
        if ty.is_zero_sized() {
            template.push(PrefabTemplateValue {
                stable_hash,
                is_buffer: false,
                enabled: is_enabled,
                bytes: Vec::new(),
                reference_patches: Vec::new(),
                external_patches: Vec::new(),
            });
            continue;
        }
        let bytes = if ty.is_buffer() {
            resolved.buffer_payload(
                buffers,
                chunk,
                row,
                stable_hash,
                usize::try_from(layout.spec.element_size)?,
            )?
        } else {
            resolved.component_bytes(chunk, row, stable_hash)?
        }
        .context("selected prefab component bytes missing")?;
        let stride = usize::try_from(if ty.is_buffer() {
            layout.spec.element_size
        } else {
            layout.spec.size_in_chunk
        })?;
        let Some(patches) =
            group_reference_offsets(bytes, stride, &layout.entity_offsets, ordinals, external)?
        else {
            continue;
        };
        template.push(PrefabTemplateValue {
            stable_hash,
            is_buffer: ty.is_buffer(),
            enabled: is_enabled,
            bytes: bytes.to_vec(),
            reference_patches: patches.local,
            external_patches: patches.external,
        });
    }
    Ok(template)
}

fn read_entity(bytes: &[u8]) -> Result<Entity> {
    ensure!(bytes.len() == 8, "invalid prefab entity handle size");
    Ok(Entity::new(
        i32::from_le_bytes(bytes[..4].try_into()?),
        i32::from_le_bytes(bytes[4..].try_into()?),
    ))
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ReferencePatches {
    local: Vec<(usize, usize)>,
    external: Vec<(usize, PrefabMemberKey)>,
}

fn group_reference_offsets(
    bytes: &[u8],
    stride: usize,
    offsets: &[i32],
    ordinals: &rustc_hash::FxHashMap<Entity, usize>,
    external: Option<&rustc_hash::FxHashMap<Entity, PrefabMemberKey>>,
) -> Result<Option<ReferencePatches>> {
    ensure!(
        stride > 0 && bytes.len().is_multiple_of(stride),
        "invalid prefab component stride"
    );
    let mut patches = ReferencePatches::default();
    for (index, element) in bytes.chunks_exact(stride).enumerate() {
        for &offset in offsets {
            let offset = usize::try_from(offset)?;
            let source = read_entity(
                element
                    .get(offset..offset + 8)
                    .context("prefab entity field outside element")?,
            )?;
            if source == Entity::NULL {
                continue;
            }
            let offset = index * stride + offset;
            if let Some(&ordinal) = ordinals.get(&source) {
                patches.local.push((offset, ordinal));
            } else if let Some(&key) = external.and_then(|index| index.get(&source)) {
                patches.external.push((offset, key));
            } else {
                return Ok(None);
            }
        }
    }
    Ok(Some(patches))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefab_root_reference_patch_plan_checks_all_buffer_elements_and_versions() -> Result<()> {
        let root = Entity::new(114, 2);
        let ordinals = rustc_hash::FxHashMap::from_iter([(root, 0)]);
        let mut buffer = [0; 24];
        buffer[16..20].copy_from_slice(&root.index.to_le_bytes());
        buffer[20..24].copy_from_slice(&root.version.to_le_bytes());
        assert_eq!(
            group_reference_offsets(&buffer, 12, &[4], &ordinals, None)?,
            Some(ReferencePatches {
                local: vec![(16, 0)],
                external: vec![]
            })
        );
        buffer[20..24].copy_from_slice(&1i32.to_le_bytes());
        assert_eq!(
            group_reference_offsets(&buffer, 12, &[4], &ordinals, None)?,
            None
        );
        assert!(group_reference_offsets(&buffer, 12, &[5], &ordinals, None).is_err());
        Ok(())
    }
}

/// Blob payloads recovered from matching saved prefab members and current server scenes.
/// Saved addresses are identifiers and are never dereferenced.
#[derive(Debug, Default)]
pub struct SavedPrefabBlobs {
    pub payloads: std::collections::HashMap<u64, (u64, Vec<u8>)>,
    pub matched_fields: usize,
    pub nonnull_fields: usize,
    pub missing_fields: usize,
    pub unbound_fields: usize,
    pub conflicts: usize,
    pub null_mismatches: usize,
}

impl ScenePrefabCatalog {
    pub fn read_saved_prefab_blobs(
        data_dir: &std::path::Path,
        layouts: &TypeLayouts,
        header: &crate::persistence::PersistenceHeader,
    ) -> Result<SavedPrefabBlobs> {
        use std::collections::HashMap;
        let data = data_dir.join("StreamingAssets/EntityScenes");
        let saved: HashMap<_, _> = header
            .prefabs
            .iter()
            .map(|prefab| (prefab.prefab_guid, prefab))
            .collect();
        let mut sources = HashMap::new();
        for blob in &header.blobs {
            sources
                .entry(blob.entity)
                .or_insert_with(Vec::new)
                .push(blob);
        }
        let mut blob_identities = HashMap::new();
        let mut conflicts = 0;
        let mut null_mismatches = 0;
        let mut found = 0;
        let mut nonnull = 0;
        let mut missing = 0;
        for (scene_id, _) in SERVER_PREFAB_SCENES {
            let scene = SceneFile::open(&data.join(format!("{scene_id}.0.entities")))?;
            let catalog = ScenePrefabCatalog::read(&scene, layouts)?;
            let resolved = layouts.resolve_scene(&scene)?;
            let buffers = scene.buffer_data()?;
            let blobs = scene.blob_assets()?;
            let mut locations = HashMap::new();
            for (ci, chunk) in resolved.chunks.iter().enumerate() {
                for row in 0..usize::try_from(chunk.header.count)? {
                    if let Some(bytes) = resolved.component_bytes(ci, row, ENTITY_HASH)? {
                        let entity = Entity::new(
                            i32::from_le_bytes(bytes[..4].try_into()?),
                            i32::from_le_bytes(bytes[4..8].try_into()?),
                        );
                        locations.insert(entity, (ci, row));
                    }
                }
            }
            for prefab in &catalog.prefabs {
                let Some(old) = saved.get(&prefab.prefab_guid) else {
                    continue;
                };
                let linked = old.linked_entities.len() == prefab.linked_entities.len();
                let members = std::iter::once((old.entity, prefab.entity)).chain(
                    old.linked_entities
                        .iter()
                        .copied()
                        .zip(prefab.linked_entities.iter().copied())
                        .filter(|_| linked),
                );
                for (source, current) in members {
                    let Some(records) = sources.remove(&source) else {
                        continue;
                    };
                    let &(ci, row) = locations.get(&current).context("missing prefab member")?;
                    for record in records {
                        let Some(layout) = layouts.get(record.component_stable_type_hash) else {
                            missing += 1;
                            continue;
                        };
                        if layout
                            .blob_offsets
                            .as_ref()
                            .is_some_and(|offsets| !offsets.contains(&record.blob_field_offset))
                        {
                            missing += 1;
                            continue;
                        }
                        let bytes = if layout.spec.category == TypeCategory::BufferData {
                            resolved
                                .buffer_payload(
                                    &buffers,
                                    ci,
                                    row,
                                    record.component_stable_type_hash,
                                    usize::try_from(layout.spec.element_size)?,
                                )?
                                .and_then(|payload| {
                                    let start = usize::try_from(record.element_index)
                                        .ok()?
                                        .checked_mul(layout.spec.element_size as usize)?;
                                    let end =
                                        start.checked_add(layout.spec.element_size as usize)?;
                                    payload.get(start..end)
                                })
                        } else {
                            resolved.component_bytes(ci, row, record.component_stable_type_hash)?
                        };
                        let Some(value) = bytes.and_then(|bytes| {
                            usize::try_from(record.blob_field_offset)
                                .ok()
                                .and_then(|offset| bytes.get(offset..offset.checked_add(4)?))
                        }) else {
                            missing += 1;
                            continue;
                        };
                        let offset = i32::from_le_bytes(value.try_into()?);
                        if offset == -1 || blobs.find_by_offset(offset).is_some() {
                            if let Some(blob) = blobs.find_by_offset(offset) {
                                if record.saved_blob_asset_address == 0 {
                                    null_mismatches += 1;
                                } else {
                                    let identity = (blob.hash, blob.data.to_vec());
                                    if let Some(previous) = blob_identities
                                        .insert(record.saved_blob_asset_address, identity.clone())
                                    {
                                        conflicts += usize::from(previous != identity);
                                    }
                                }
                            } else if record.saved_blob_asset_address != 0 {
                                null_mismatches += 1;
                            }
                            found += 1;
                            nonnull += usize::from(offset != -1);
                        } else {
                            missing += 1;
                        }
                    }
                }
            }
        }
        ensure!(conflicts == 0 && null_mismatches == 0, "conflicting saved prefab blob mappings: {conflicts} conflicts, {null_mismatches} null mismatches");
        Ok(SavedPrefabBlobs {
            payloads: blob_identities,
            matched_fields: found,
            nonnull_fields: nonnull,
            missing_fields: missing,
            unbound_fields: sources.values().map(Vec::len).sum(),
            conflicts,
            null_mismatches,
        })
    }
}
