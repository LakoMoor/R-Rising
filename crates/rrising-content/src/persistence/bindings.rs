use anyhow::{ensure, Context, Result};
use rrising_ecs::Entity;
use rustc_hash::FxHashMap;

use super::{PrefabReference, SavedSnapshot};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SavedBindingIssue {
    MissingPrefab(i32),
    LinkedCount {
        guid: i32,
        saved: usize,
        current: usize,
    },
    ConflictingTarget {
        saved: Entity,
        kept: Entity,
        rejected: Entity,
    },
    ConflictingSource {
        current: Entity,
        kept: Entity,
        rejected: Entity,
    },
}

/// The two native mapping directions keep their first entry independently.
#[derive(Debug, Default)]
pub struct SavedEntityBindings {
    targets: FxHashMap<Entity, Entity>,
    sources: FxHashMap<Entity, Entity>,
    issues: Vec<SavedBindingIssue>,
}

impl SavedEntityBindings {
    pub fn target(&self, saved: Entity) -> Option<Entity> {
        self.targets.get(&saved).copied()
    }

    pub fn source(&self, current: Entity) -> Option<Entity> {
        self.sources.get(&current).copied()
    }

    pub fn len(&self) -> usize {
        self.targets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    pub fn issues(&self) -> &[SavedBindingIssue] {
        &self.issues
    }

    fn bind(&mut self, saved: Entity, current: Entity) {
        let kept = *self.targets.entry(saved).or_insert(current);
        if kept != current {
            self.issues.push(SavedBindingIssue::ConflictingTarget {
                saved,
                kept,
                rejected: current,
            });
        }
        let kept = *self.sources.entry(current).or_insert(saved);
        if kept != saved {
            self.issues.push(SavedBindingIssue::ConflictingSource {
                current,
                kept,
                rejected: saved,
            });
        }
    }

    fn bind_prefabs(
        &mut self,
        saved_prefabs: &[PrefabReference],
        prefabs: &[PrefabReference],
    ) -> Result<()> {
        let mut current = FxHashMap::default();
        for prefab in prefabs {
            ensure!(
                current.insert(prefab.prefab_guid, prefab).is_none(),
                "duplicate current prefab GUID {}",
                prefab.prefab_guid
            );
        }
        for saved in saved_prefabs {
            let guid = saved.prefab_guid;
            let Some(current) = current.get(&guid) else {
                self.issues.push(SavedBindingIssue::MissingPrefab(guid));
                continue;
            };
            self.bind(saved.entity, current.entity);
            let saved_count = saved.linked_entities.len();
            let current_count = current.linked_entities.len();
            if saved_count != current_count {
                self.issues.push(SavedBindingIssue::LinkedCount {
                    guid,
                    saved: saved_count,
                    current: current_count,
                });
                continue;
            }
            for (&saved, &current) in saved
                .linked_entities
                .iter()
                .zip(&current.linked_entities)
                .skip(1)
            {
                self.bind(saved, current);
            }
        }
        Ok(())
    }
}

impl SavedSnapshot<'_> {
    /// Pairs job rows with caller-allocated handles in job, archetype, chunk and row order.
    /// Current prefab groups must include the root entry at ordinal zero, when present.
    pub fn bind_entities(
        &self,
        fresh: impl IntoIterator<Item = Entity>,
        prefabs: &[PrefabReference],
    ) -> Result<SavedEntityBindings> {
        let mut bindings = SavedEntityBindings::default();
        let mut fresh = fresh.into_iter();
        for saved in self.entities_in_job_order() {
            let current = fresh
                .next()
                .context("missing fresh handle for saved entity")?;
            ensure!(current.index > 0, "invalid fresh entity handle");
            bindings.bind(saved, current);
        }
        ensure!(fresh.next().is_none(), "surplus fresh entity handles");
        bindings.bind_prefabs(&self.header().prefabs, prefabs)?;
        bindings.bind(Entity::NULL, Entity::NULL);
        Ok(bindings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handle(index: i32) -> Entity {
        Entity::new(index, 1)
    }

    fn prefab(guid: i32, root: i32, linked: &[i32]) -> PrefabReference {
        PrefabReference {
            prefab_guid: guid,
            entity: handle(root),
            linked_entities: linked.iter().copied().map(handle).collect(),
        }
    }

    #[test]
    fn prefab_binding_checks_guid_count_and_linked_ordinal() -> Result<()> {
        let saved = [
            prefab(-1, 10, &[10, 11, 12]),
            prefab(2, 20, &[20, 21]),
            prefab(3, 30, &[]),
        ];
        let current = [prefab(2, 200, &[]), prefab(-1, 100, &[100, 102, 101])];
        let mut bindings = SavedEntityBindings::default();
        bindings.bind_prefabs(&saved, &current)?;
        assert_eq!(bindings.target(handle(10)), Some(handle(100)));
        assert_eq!(bindings.target(handle(11)), Some(handle(102)));
        assert_eq!(bindings.source(handle(101)), Some(handle(12)));
        assert_eq!(bindings.target(Entity::new(12, 2)), None);
        assert_eq!(bindings.target(handle(20)), Some(handle(200)));
        assert_eq!(bindings.target(handle(21)), None);
        assert_eq!(bindings.target(handle(30)), None);
        assert_eq!(
            bindings.issues(),
            &[
                SavedBindingIssue::LinkedCount {
                    guid: 2,
                    saved: 2,
                    current: 0
                },
                SavedBindingIssue::MissingPrefab(3),
            ]
        );
        Ok(())
    }

    #[test]
    fn conflicting_directions_keep_their_first_values_independently() {
        let mut bindings = SavedEntityBindings::default();
        bindings.bind(handle(1), handle(10));
        bindings.bind(handle(1), handle(10));
        assert!(bindings.issues().is_empty());
        bindings.bind(handle(1), handle(11));
        bindings.bind(handle(2), handle(10));
        assert_eq!(bindings.target(handle(1)), Some(handle(10)));
        assert_eq!(bindings.target(handle(2)), Some(handle(10)));
        assert_eq!(bindings.source(handle(10)), Some(handle(1)));
        assert_eq!(bindings.source(handle(11)), Some(handle(1)));
        assert_eq!(bindings.issues().len(), 2);
    }
}
