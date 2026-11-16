//! `Unity.Entities.Entity` — the handle every gameplay system passes around.
//!
//! Layout from `dump.cs` (`public struct Entity : IEquatable<Entity>, IComparable<Entity>`):
//! `int Index; // 0x0` and `int Version; // 0x4`, eight bytes total. `Index` is a slot in the
//! world's entity array and `Version` distinguishes a slot that has been recycled.
//!
//! The two members disagree about what "ordered" means in the original, and the port keeps that
//! disagreement instead of tidying it away: `Equals` compares `Index` *and* `Version`, while
//! `CompareTo` subtracts `Index` and nothing else.

use std::cmp::Ordering;
use std::hash::{Hash, Hasher};

/// `Unity.Entities.Entity` — `Index` at 0x0, `Version` at 0x4 (`dump.cs`).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Entity {
    /// `Entity.Index` — the slot this handle refers to.
    pub index: i32,
    /// `Entity.Version` — bumped when the slot is reused.
    pub version: i32,
}

impl Entity {
    /// `Entity.Null` — the handle with `Index` 0 and `Version` 0.
    ///
    /// src: `Entity.get_Null` @ 0x173f700 (IL2CPP filed that body under
    /// `CustomPostProcessVolumeComponent.get_injectionPoint`, which shares the code) and
    /// `Entity.ToString` @ 0x8fc5b20, whose first test is `Index == 0 && Version == 0` — the
    /// original treats that pair as the null handle.
    pub const NULL: Entity = Entity { index: 0, version: 0 };

    /// The two-field construction the dump lists (`Entity(int index, int version)`).
    #[inline]
    pub const fn new(index: i32, version: i32) -> Self {
        Self { index, version }
    }

    /// `Entity.CompareTo(Entity)` — src: `Unity_Entities__Unity_Entities__Entity___IEquatable_Entity___IComparable_Entity___CompareTo_2`
    /// @ 0x8fc5a80, whose whole body is `return *param_1 - param_2;`: the difference of the two
    /// indices, `Version` not consulted. The subtraction is unchecked in C#, so the port wraps.
    #[inline]
    pub fn compare_to(self, other: Self) -> i32 {
        self.index.wrapping_sub(other.index)
    }
}

/// `Entity.op_Equality` / `Entity.Equals(Entity)` — src: @ 0x784e1b0 (filed under
/// `TrailSystemID.op_Equality`, same compiled code) and @ 0x79a6050 (filed under
/// `ModifiableEntity.Equals`): compare `Index`, and only if it matches compare `Version`.
impl PartialEq for Entity {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.version == other.version
    }
}

impl Eq for Entity {}

/// `Entity.CompareTo` orders by `Index` alone, so two handles for the same slot with different
/// versions order as equal while `==` says they differ. That is the original's behaviour, not an
/// accident of this port.
impl Ord for Entity {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.index.cmp(&other.index)
    }
}

impl PartialOrd for Entity {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// `Entity.GetHashCode` — src: @ 0x17d8050 (filed under `AttachedDepth.GetHashCode`, same compiled
/// code): the body returns the first field, which for `Entity` is `Index`.
impl Hash for Entity {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_i32(self.index);
    }
}
