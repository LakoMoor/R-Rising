use anyhow::{ensure, Context, Result};
use rrising_ecs::Entity;

use crate::entity_scenes::SceneFile;
use crate::persistence::PrefabReference;
use crate::type_layouts::{TypeLayouts, ENTITY_HASH};

pub const REGISTER_PREFAB_HASH: u64 = 0xa965_c72e_229a_11cf;
pub const REGISTER_PREFAB_EVENT_HASH: u64 = 0x340f_ac1f_a9e0_4679;
pub const LINKED_ENTITY_GROUP_HASH: u64 = 0xbea7_9ca4_5e4b_e521;

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
}

impl ScenePrefabCatalog {
    pub fn read_server_game_data(
        data_dir: &std::path::Path,
        layouts: &TypeLayouts,
    ) -> Result<Vec<Self>> {
        let directory = data_dir.join("StreamingAssets/EntityScenes");
        SERVER_PREFAB_SCENES
            .iter()
            .map(|(scene_id, expected)| {
                let scene = SceneFile::open(&directory.join(format!("{scene_id}.0.entities")))?;
                let catalog = Self::read(&scene, layouts)
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
