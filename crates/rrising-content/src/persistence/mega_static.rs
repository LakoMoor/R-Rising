use anyhow::{ensure, Context, Result};
use rrising_ecs::Entity;
use rustc_hash::{FxHashMap, FxHashSet};

use super::SavedSnapshot;

const INSTANCE_HASH: u64 = 0xaf63_54eb_2c92_3275;
const MANAGER_HASH: u64 = 0x4fff_c141_e07f_ea9d;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MegaStaticRestoreEntry {
    pub saved_manager: Entity,
    pub buffer_ordinal: usize,
    pub saved_instance: Entity,
    pub prefab_guid: i32,
    pub saved_prefab_root: Entity,
    pub terrain_chunk: u16,
    pub transform_index: u16,
}

#[derive(Debug, Default)]
pub struct MegaStaticRestorePlan {
    pub entries: Vec<MegaStaticRestoreEntry>,
    pub manager_rows: usize,
    pub vacant_slots: usize,
    pub distinct_prefab_guids: usize,
}

impl MegaStaticRestorePlan {
    /// Resolves the persistent placement identities. Transforms require the loaded
    /// server's static-transform lookup; a plan alone does not instantiate anything.
    pub fn read(snapshot: &SavedSnapshot<'_>) -> Result<Self> {
        let header = snapshot.header();
        let component = header
            .persistent_component_named("ProjectM.MegaStaticInstanceBuffer")
            .context("missing saved MegaStatic instance layout")?;
        ensure!(
            component.stable_type_hash == INSTANCE_HASH
                && component.component_size == 12
                && component.saved_type_index.is_buffer(),
            "unexpected MegaStatic buffer layout"
        );
        let manager = header
            .persistent_component_named("ProjectM.MegaStaticManager")
            .context("missing saved MegaStatic manager layout")?;
        ensure!(
            manager.stable_type_hash == MANAGER_HASH
                && manager.component_size == 12
                && !manager.saved_type_index.is_buffer(),
            "unexpected MegaStatic manager layout"
        );
        let mut prefab_by_guid = FxHashMap::default();
        for prefab in &header.prefabs {
            ensure!(
                prefab_by_guid
                    .insert(prefab.prefab_guid, prefab.entity)
                    .is_none(),
                "duplicate saved prefab GUID {}",
                prefab.prefab_guid
            );
        }
        let mut result = Self::default();
        let mut seen = FxHashSet::default();
        let mut guids = FxHashSet::default();
        for (_, job) in snapshot.jobs() {
            for chunk in job.archetypes.iter().flat_map(|arch| &arch.chunks) {
                let Some(column) = chunk
                    .columns
                    .iter()
                    .find(|column| column.saved_type_index == component.saved_type_index)
                else {
                    continue;
                };
                let manager_column = chunk
                    .columns
                    .iter()
                    .find(|column| column.saved_type_index == manager.saved_type_index)
                    .context("instance buffer without saved MegaStatic manager")?;
                for (row, (saved_manager, bytes)) in chunk
                    .entities()
                    .zip(column.buffers().context("expected MegaStatic buffer")?)
                    .enumerate()
                {
                    let manager_bytes = manager_column
                        .scalar(row)
                        .context("missing MegaStatic manager row")?;
                    let terrain_chunk = u16::from_le_bytes(manager_bytes[..2].try_into()?);
                    let start_index = u16::from_le_bytes(manager_bytes[2..4].try_into()?);
                    result.manager_rows += 1;
                    for (buffer_ordinal, element) in bytes.as_chunks::<12>().0.iter().enumerate() {
                        let saved_instance = Entity::new(
                            i32::from_le_bytes(element[..4].try_into()?),
                            i32::from_le_bytes(element[4..8].try_into()?),
                        );
                        let prefab_guid = i32::from_le_bytes(element[8..12].try_into()?);
                        if prefab_guid == 0 {
                            ensure!(
                                saved_instance == Entity::NULL,
                                "invalid vacant MegaStatic slot"
                            );
                            result.vacant_slots += 1;
                            continue;
                        }
                        let index = usize::try_from(saved_instance.index)?;
                        ensure!(
                            index > 0
                                && snapshot.world_data().entity_version(index)
                                    == Some(saved_instance.version),
                            "invalid saved MegaStatic instance {saved_instance:?}"
                        );
                        ensure!(
                            seen.insert(saved_instance),
                            "duplicate saved MegaStatic instance"
                        );
                        let saved_prefab_root =
                            *prefab_by_guid.get(&prefab_guid).with_context(|| {
                                format!("unknown saved MegaStatic prefab GUID {prefab_guid}")
                            })?;
                        guids.insert(prefab_guid);
                        result.entries.push(MegaStaticRestoreEntry {
                            saved_manager,
                            buffer_ordinal,
                            saved_instance,
                            prefab_guid,
                            saved_prefab_root,
                            terrain_chunk,
                            transform_index: start_index
                                .wrapping_add(u16::try_from(buffer_ordinal)?),
                        });
                    }
                }
            }
        }
        result.distinct_prefab_guids = guids.len();
        Ok(result)
    }
}
