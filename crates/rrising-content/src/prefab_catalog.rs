use std::collections::{BTreeMap, BTreeSet};

use anyhow::{ensure, Context, Result};
use rrising_ecs::{Entity, TypeCategory, TypeIndex};

use crate::entity_scenes::SceneFile;
use crate::persistence::PrefabReference;
use crate::type_layouts::{
    TypeLayouts, ENTITY_HASH, LOCAL_TO_WORLD_HASH, LOCAL_TRANSFORM_HASH, ROTATION_HASH,
    TRANSLATION_HASH,
};

pub const REGISTER_PREFAB_HASH: u64 = 0xa965_c72e_229a_11cf;
pub const REGISTER_PREFAB_EVENT_HASH: u64 = 0x340f_ac1f_a9e0_4679;
pub const LINKED_ENTITY_GROUP_HASH: u64 = 0xbea7_9ca4_5e4b_e521;
const STATIC_INDEX_HASH: u64 = 0xdd17_1d94_8ccb_47ea;
const PREFAB_GUID_HASH: u64 = 0xb181_3197_8820_0ae1;
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
}

#[derive(Debug)]
pub struct PrefabTemplateValue {
    pub stable_hash: u64,
    pub is_buffer: bool,
    pub bytes: Vec<u8>,
}

impl ScenePrefabCatalog {
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
                let catalog = Self::read_with_templates(&scene, layouts, requested)
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
        Self::read_with_templates(scene, layouts, &BTreeSet::new())
    }

    fn read_with_templates(
        scene: &SceneFile,
        layouts: &TypeLayouts,
        requested: &BTreeSet<i32>,
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
            let linked =
                resolved.buffer_payload(&buffers, chunk, row, LINKED_ENTITY_GROUP_HASH, 8)?;
            prefab.linked_entities = linked
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
            if !requested.contains(&prefab.prefab_guid) {
                continue;
            }
            let arch = resolved.archetypes[resolved.chunks[chunk].header.archetype_index as usize]
                .as_ref()
                .context("unresolved prefab archetype")?;
            let mut template = Vec::new();
            for (&stable_hash, &ty) in arch.hashes.iter().zip(&arch.types) {
                if [
                    ENTITY_HASH,
                    PREFAB_GUID_HASH,
                    STATIC_INDEX_HASH,
                    TRANSLATION_HASH,
                    ROTATION_HASH,
                    LOCAL_TO_WORLD_HASH,
                    LOCAL_TRANSFORM_HASH,
                    LINKED_ENTITY_GROUP_HASH,
                    REGISTER_PREFAB_HASH,
                    REGISTER_PREFAB_EVENT_HASH,
                    REGISTER_COLLECTION_HASH,
                ]
                .contains(&stable_hash)
                {
                    continue;
                }
                let layout = layouts
                    .get(stable_hash)
                    .context("missing prefab component layout")?;
                if !layout.portable_value
                    || !layout.entity_offsets.is_empty()
                    || layout.name.starts_with("ProjectM.Network.")
                    || ty.is_chunk_component()
                    || ty.is_zero_sized()
                    || ty.is_enableable()
                    || ty.is_cleanup_component()
                    || !matches!(
                        layout.spec.category,
                        TypeCategory::ComponentData | TypeCategory::BufferData
                    )
                    || ty.value as u32 & (TypeIndex::K_MANAGED_MASK | TypeIndex::K_CLEANUP_MASK)
                        != 0
                {
                    continue;
                }
                let bytes = if ty.is_buffer() {
                    resolved.buffer_payload(
                        &buffers,
                        chunk,
                        row,
                        stable_hash,
                        usize::try_from(layout.spec.element_size)?,
                    )?
                } else {
                    resolved.component_bytes(chunk, row, stable_hash)?
                }
                .context("selected prefab component bytes missing")?;
                template.push(PrefabTemplateValue {
                    stable_hash,
                    is_buffer: ty.is_buffer(),
                    bytes: bytes.to_vec(),
                });
            }
            ensure!(
                result
                    .instance_templates
                    .insert(prefab.prefab_guid, template)
                    .is_none(),
                "duplicate prefab template GUID"
            );
        }
        result
            .prefabs
            .retain(|prefab| locations.contains_key(&prefab.entity));
        result.unresolved_rows += missing_roots;
        Ok(result)
    }
}

fn read_entity(bytes: &[u8]) -> Result<Entity> {
    ensure!(bytes.len() == 8, "invalid prefab entity handle size");
    Ok(Entity::new(
        i32::from_le_bytes(bytes[..4].try_into()?),
        i32::from_le_bytes(bytes[4..].try_into()?),
    ))
}
