//! Bounded movement-direction selection for command creation.
//!
//! Assumes a valid enabled controlled entity; controller validation, ECS lookups,
//! network command construction, and command playback are outside this function.

use anyhow::{ensure, Result};

/// Size of `InputCommandBufferElement` for the pinned game build.
pub const INPUT_COMMAND_SIZE: usize = 152;

/// Selected fields of an already existing command buffer element.
/// Other fields, including the complete raw input, stay in the original bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CommandMovement {
    pub direction: [f32; 2],
    pub end_time: f64,
    pub delta_time: f32,
}

impl CommandMovement {
    pub fn read(bytes: &[u8]) -> Result<Self> {
        ensure!(
            bytes.len() == INPUT_COMMAND_SIZE,
            "InputCommandBufferElement needs 152 bytes"
        );
        let f = |offset| {
            f32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("checked size"))
        };
        Ok(Self {
            direction: [f(0), f(4)],
            end_time: f64::from_le_bytes(bytes[136..144].try_into().expect("checked size")),
            delta_time: f(144),
        })
    }

    /// Pinned server `InputCommandBufferElement.get_StartTime` converts DeltaTime to
    /// double before subtracting it from EndTime (RVA 0x7939a90).
    pub fn start_time(&self) -> f64 {
        self.end_time - f64::from(self.delta_time)
    }
}

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
