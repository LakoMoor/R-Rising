//! Bounded movement-direction selection for command creation.
//!
//! Assumes a valid enabled controlled entity; controller validation, ECS lookups,
//! network command construction, and command playback are outside this function.

/// Pick the movement direction from a completed set of lookup results.
/// `spell_controlled` is `(MovementControlled, RotationControlled)` when present.
pub fn select_movement_direction(
    raw_input: [f32; 2],
    movement_input: Option<[f32; 2]>,
    spell_controlled: Option<(bool, bool)>,
) -> [f32; 2] {
    match movement_input {
        None => raw_input,
        Some(_) if spell_controlled.is_some_and(|(movement, rotation)| movement || rotation) => {
            [0.0; 2]
        }
        Some(input) => input,
    }
}
