//! Host-to-ECS input boundary for the recovered ProjectM.EntityInput layout.
//!
//! This writes commands only. Player selection, camera projection, input bindings, movement,
//! animation and the original client input pipeline are not implemented by this module.
use anyhow::{ensure, Result};
use rrising_ecs::{Entity, TypeIndex, World};

pub const ENTITY_INPUT_SIZE: usize = 96;

/// Values the host has already resolved into the original input's world-space coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerCommand {
    pub movement: [f32; 2],
    pub aim_position: [f32; 3],
    pub aim_position_plane: [f32; 3],
}

/// Update the recovered first 32 bytes, retaining all other action and input state.
/// Field layout is corroborated by the original shared steering replays in
/// `evidence/oracles/movement_shared.tsv`; this function is not an original client input replay.
pub fn write_player_command(
    world: &mut World,
    entity: Entity,
    ty: TypeIndex,
    command: PlayerCommand,
) -> Result<()> {
    let original = world.store.component_bytes(entity, ty)?;
    ensure!(
        original.len() == ENTITY_INPUT_SIZE,
        "EntityInput needs 96 bytes"
    );
    let mut bytes = original.to_vec();
    for (out, value) in bytes[..32].as_chunks_mut::<4>().0.iter_mut().zip(
        command
            .movement
            .into_iter()
            .chain(command.aim_position)
            .chain(command.aim_position_plane),
    ) {
        out.copy_from_slice(&value.to_le_bytes());
    }
    world.store.set_component_bytes(entity, ty, &bytes)
}

/// Update only movement axes; aim and action fields remain unchanged.
pub fn write_player_movement(
    world: &mut World,
    entity: Entity,
    ty: TypeIndex,
    movement: [f32; 2],
) -> Result<()> {
    let original = world.store.component_bytes(entity, ty)?;
    ensure!(
        original.len() == ENTITY_INPUT_SIZE,
        "EntityInput needs 96 bytes"
    );
    let mut bytes = original.to_vec();
    for (out, value) in bytes[..8].as_chunks_mut::<4>().0.iter_mut().zip(movement) {
        out.copy_from_slice(&value.to_le_bytes());
    }
    world.store.set_component_bytes(entity, ty, &bytes)
}
