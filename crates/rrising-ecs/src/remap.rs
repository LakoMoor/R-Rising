//! Entity-reference remapping, the semantics `EntityRemapUtility` applies when a serialized scene
//! is loaded into a world.
//!
//! Provenance: the REA server did not respond for the pinned `GameAssembly` in this session, so
//! the body of `EntityRemapUtility.RemapEntity` is *not* re-read here. The rule below is the
//! documented Unity one (`Entities` 1.x): a table indexed by the source entity index holds
//! `(SourceVersion, Target)`; a reference resolves to `Target` when its version equals
//! `SourceVersion`, otherwise to `Entity.Null`. It is validated against data invariants (see
//! `docs/COMPATIBILITY.md`), not against native replay. The patch locations (`EntityPatchInfo`
//! offsets) come from the recovered type layouts.

use crate::entity::Entity;

/// `EntityRemapUtility.EntityRemapInfo`: one slot per source entity index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RemapInfo {
    source_version: i32,
    target: Entity,
}

/// What a stored reference turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefClass {
    /// `Entity.Null` (index 0, version 0).
    Null,
    /// Names a registered source entity with a matching version.
    Live,
    /// Index registered but the version differs (a stale handle).
    StaleVersion,
    /// Index not registered at all.
    Dangling,
}

/// Source-entity to target-entity table. Unregistered slots behave like the original's zeroed
/// table: `(SourceVersion 0, Target Null)`, so `Entity.Null` maps to itself.
#[derive(Clone, Debug, Default)]
pub struct EntityRemapTable {
    entries: Vec<RemapInfo>,
    registered: Vec<bool>,
}

impl EntityRemapTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// `AddEntityRemapping`: `source` (as stored) now resolves to `target`.
    pub fn add(&mut self, source: Entity, target: Entity) {
        let Ok(i) = usize::try_from(source.index) else {
            return;
        };
        if i >= self.entries.len() {
            self.entries.resize(i + 1, RemapInfo::default());
            self.registered.resize(i + 1, false);
        }
        self.entries[i] = RemapInfo {
            source_version: source.version,
            target,
        };
        self.registered[i] = true;
    }

    pub fn classify(&self, source: Entity) -> RefClass {
        if source == Entity::NULL {
            return RefClass::Null;
        }
        match usize::try_from(source.index) {
            Ok(i) if i < self.entries.len() && self.registered[i] => {
                if self.entries[i].source_version == source.version {
                    RefClass::Live
                } else {
                    RefClass::StaleVersion
                }
            }
            _ => RefClass::Dangling,
        }
    }

    /// `RemapEntity`: the target when the version matches, `Entity.Null` otherwise.
    pub fn remap(&self, source: Entity) -> Entity {
        match usize::try_from(source.index) {
            Ok(i) if i < self.entries.len() && self.entries[i].source_version == source.version => {
                self.entries[i].target
            }
            _ => Entity::NULL,
        }
    }
}

/// Counts of classified references.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RefCounts {
    pub null: usize,
    pub live: usize,
    pub stale_version: usize,
    pub dangling: usize,
}

impl RefCounts {
    pub fn total(&self) -> usize {
        self.null + self.live + self.stale_version + self.dangling
    }
    pub fn add(&mut self, c: RefClass) {
        match c {
            RefClass::Null => self.null += 1,
            RefClass::Live => self.live += 1,
            RefClass::StaleVersion => self.stale_version += 1,
            RefClass::Dangling => self.dangling += 1,
        }
    }
}

fn read_entity(b: &[u8], at: usize) -> Option<Entity> {
    let s = b.get(at..at + 8)?;
    Some(Entity::new(
        i32::from_le_bytes(s[0..4].try_into().ok()?),
        i32::from_le_bytes(s[4..8].try_into().ok()?),
    ))
}

fn write_entity(b: &mut [u8], at: usize, e: Entity) {
    b[at..at + 4].copy_from_slice(&e.index.to_le_bytes());
    b[at + 4..at + 8].copy_from_slice(&e.version.to_le_bytes());
}

/// Classify (and with `patch` rewrite) every `Entity` field of an array of `stride`-byte elements
/// (`offsets` are byte offsets inside one element). Fields that fall outside the slice are ignored.
pub fn remap_elements(
    bytes: &mut [u8],
    stride: usize,
    offsets: &[i32],
    table: &EntityRemapTable,
    patch: bool,
    counts: &mut RefCounts,
) {
    if stride == 0 {
        return;
    }
    for base in (0..bytes.len() / stride).map(|i| i * stride) {
        for &o in offsets {
            let at = base + o as usize;
            if let Some(e) = read_entity(bytes, at) {
                counts.add(table.classify(e));
                if patch {
                    write_entity(bytes, at, table.remap(e));
                }
            }
        }
    }
}

/// Read-only variant of [`remap_elements`].
pub fn classify_elements(
    bytes: &[u8],
    stride: usize,
    offsets: &[i32],
    table: &EntityRemapTable,
    counts: &mut RefCounts,
) {
    if stride == 0 {
        return;
    }
    for base in (0..bytes.len() / stride).map(|i| i * stride) {
        for &o in offsets {
            if let Some(e) = read_entity(bytes, base + o as usize) {
                counts.add(table.classify(e));
            }
        }
    }
}

/// Result of walking a parent relation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HierarchyReport {
    pub with_parent: usize,
    pub null_parent: usize,
    /// Parent handle does not name a live entity.
    pub dangling_parent: usize,
    /// Entities whose parent chain revisits an entity.
    pub cyclic: usize,
    pub max_depth: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_follows_version_rule() {
        let mut t = EntityRemapTable::new();
        t.add(Entity::new(3, 2), Entity::new(10, 5));
        assert_eq!(t.remap(Entity::new(3, 2)), Entity::new(10, 5));
        assert_eq!(t.remap(Entity::new(3, 1)), Entity::NULL);
        assert_eq!(t.remap(Entity::new(99, 1)), Entity::NULL);
        assert_eq!(t.classify(Entity::NULL), RefClass::Null);
        assert_eq!(t.classify(Entity::new(3, 2)), RefClass::Live);
        assert_eq!(t.classify(Entity::new(3, 1)), RefClass::StaleVersion);
        assert_eq!(t.classify(Entity::new(4, 1)), RefClass::Dangling);
    }

    #[test]
    fn patches_fields_inside_elements() {
        let mut t = EntityRemapTable::new();
        t.add(Entity::new(1, 1), Entity::new(7, 3));
        let mut b = vec![0u8; 24];
        write_entity(&mut b, 4, Entity::new(1, 1));
        write_entity(&mut b, 16, Entity::new(5, 1));
        let mut c = RefCounts::default();
        remap_elements(&mut b, 12, &[4], &t, true, &mut c);
        assert_eq!((c.live, c.dangling), (1, 1));
        assert_eq!(read_entity(&b, 4), Some(Entity::new(7, 3)));
        assert_eq!(read_entity(&b, 16), Some(Entity::NULL));
    }
}
