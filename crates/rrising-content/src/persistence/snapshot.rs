use anyhow::{ensure, Context, Result};
use rrising_ecs::{Entity, TypeIndex};
use rustc_hash::FxHashMap;

use super::{
    PersistenceHeader, PersistentSave, SavedChunk, SavedSystems, SavedWorldData, SerializationJob,
    SerializationJobRegistry,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedEntityLocation {
    pub job: usize,
    pub archetype: usize,
    pub chunk: usize,
    pub row: usize,
}

/// Retains the original slot and version across all job members. Columns borrow the save.
#[derive(Debug)]
pub struct SavedSnapshot<'a> {
    header: PersistenceHeader,
    world_data: SavedWorldData<'a>,
    systems: SavedSystems<'a>,
    jobs: Vec<(usize, SerializationJob<'a>)>,
    slots: FxHashMap<i32, (i32, SavedEntityLocation)>,
}

#[derive(Debug)]
pub struct SavedEntityView<'view, 'save> {
    pub location: SavedEntityLocation,
    chunk: &'view SavedChunk<'save>,
}

impl<'save> SavedEntityView<'_, 'save> {
    pub fn scalar(&self, saved_type: TypeIndex) -> Option<&'save [u8]> {
        self.chunk
            .columns
            .iter()
            .find(|column| column.saved_type_index == saved_type)?
            .scalar(self.location.row)
    }

    pub fn buffer(&self, saved_type: TypeIndex) -> Option<&'save [u8]> {
        self.chunk
            .columns
            .iter()
            .find(|column| column.saved_type_index == saved_type)?
            .buffers()?
            .nth(self.location.row)
    }
}

impl<'a> SavedSnapshot<'a> {
    pub fn read(save: &'a PersistentSave) -> Result<Self> {
        let header = save.header()?;
        let world_data = save.world_data()?;
        let systems = save.systems()?;
        for prefab in &header.prefabs {
            for handle in std::iter::once(&prefab.entity).chain(&prefab.linked_entities) {
                check_world_version(&world_data, *handle)
                    .context("checking header prefab reference")?;
            }
        }
        let registry = SerializationJobRegistry::new(&header)?;
        let mut jobs = Vec::new();
        let mut slots = FxHashMap::default();
        for (member_index, member) in save.members().iter().enumerate() {
            if !member.name.starts_with("SerializationJob_") {
                continue;
            }
            let bytes = save
                .member_data(member_index)
                .context("missing serialization job data")?;
            let job = SerializationJob::read(bytes, &registry)
                .with_context(|| format!("decoding {}", member.name))?;
            for (archetype_index, archetype) in job.archetypes.iter().enumerate() {
                for (chunk_index, chunk) in archetype.chunks.iter().enumerate() {
                    for (row, entity) in chunk.entities().enumerate() {
                        ensure!(
                            entity.index > 0,
                            "invalid saved entity slot {}",
                            entity.index
                        );
                        check_world_version(&world_data, entity)
                            .with_context(|| format!("checking {}", member.name))?;
                        let location = SavedEntityLocation {
                            job: jobs.len(),
                            archetype: archetype_index,
                            chunk: chunk_index,
                            row,
                        };
                        ensure!(
                            slots
                                .insert(entity.index, (entity.version, location))
                                .is_none(),
                            "duplicate saved entity slot {} in {}",
                            entity.index,
                            member.name
                        );
                    }
                }
            }
            jobs.push((member_index, job));
        }
        Ok(Self {
            header,
            world_data,
            systems,
            jobs,
            slots,
        })
    }

    /// Pairs the index in the container table with the decoded serialization job.
    pub fn jobs(&self) -> &[(usize, SerializationJob<'a>)] {
        &self.jobs
    }

    pub fn entity_count(&self) -> usize {
        self.slots.len()
    }

    pub fn header(&self) -> &PersistenceHeader {
        &self.header
    }

    pub fn world_data(&self) -> &SavedWorldData<'a> {
        &self.world_data
    }

    pub fn systems(&self) -> &SavedSystems<'a> {
        &self.systems
    }

    pub fn entity(&self, entity: Entity) -> Option<SavedEntityView<'_, 'a>> {
        let &(version, location) = self.slots.get(&entity.index)?;
        if entity.version != version {
            return None;
        }
        let chunk =
            &self.jobs[location.job].1.archetypes[location.archetype].chunks[location.chunk];
        Some(SavedEntityView { location, chunk })
    }

    /// Enumerates indexed handles in unspecified order.
    pub fn entities(&self) -> impl ExactSizeIterator<Item = Entity> + '_ {
        self.slots
            .iter()
            .map(|(&index, &(version, _))| Entity::new(index, version))
    }
}

fn check_world_version(world: &SavedWorldData<'_>, entity: Entity) -> Result<()> {
    let index = usize::try_from(entity.index).context("negative saved entity index")?;
    let version = world.entity_version(index).with_context(|| {
        format!("saved entity slot {index} exceeds the WorldData version table")
    })?;
    ensure!(
        version == entity.version,
        "saved entity slot {index} has version {} but WorldData has {version}",
        entity.version
    );
    Ok(())
}
