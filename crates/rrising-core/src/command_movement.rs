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

/// Update the bounded input fields of an existing command after the caller has
/// validated its controller and performed the original component lookups.
/// Timing, sequence, aim and action command fields remain the caller's concern.
pub fn write_command_input(
    command: &mut [u8],
    raw_input: &[u8],
    movement_input: Option<[f32; 2]>,
    spell_controlled: Option<(bool, bool)>,
) -> Result<()> {
    ensure!(
        command.len() == INPUT_COMMAND_SIZE,
        "InputCommandBufferElement needs 152 bytes"
    );
    ensure!(raw_input.len() == 96, "EntityInput needs 96 bytes");
    let read = |offset| {
        f32::from_le_bytes(
            raw_input[offset..offset + 4]
                .try_into()
                .expect("checked size"),
        )
    };
    let direction = select_movement_direction([read(0), read(4)], movement_input, spell_controlled);
    command[..4].copy_from_slice(&direction[0].to_le_bytes());
    command[4..8].copy_from_slice(&direction[1].to_le_bytes());
    command[20..116].copy_from_slice(raw_input);
    Ok(())
}

/// Resolve the observed component lookups for an already validated controlled entity.
/// This does not implement Controller validation, timing, buffering or scheduling.
pub fn write_command_input_from_world(
    world: &rrising_ecs::World,
    entity: rrising_ecs::Entity,
    command: &mut [u8],
    input_type: rrising_ecs::TypeIndex,
) -> Result<()> {
    let raw = world.store.component_bytes(entity, input_type)?;
    let movement = world
        .types
        .type_index_of("ProjectM.Movement")
        .map(|ty| world.store.try_component_bytes(entity, ty))
        .transpose()?
        .flatten()
        .map(crate::movement::CharacterMovementFields::read)
        .transpose()?
        .map(|fields| fields.move_input);
    let spell = world
        .types
        .type_index_of("ProjectM.IsSpellControlled")
        .map(|ty| world.store.try_component_bytes(entity, ty))
        .transpose()?
        .flatten()
        .map(|bytes| {
            ensure!(bytes.len() == 2, "IsSpellControlled needs 2 bytes");
            Ok((bytes[0] != 0, bytes[1] != 0))
        })
        .transpose()?;
    write_command_input(command, raw, movement, spell)
}
