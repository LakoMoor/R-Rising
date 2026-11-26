//! An explicit ECS bridge for the isolated ordinary DoMovement arithmetic.
//!
//! This is not MoveEntitySystem or a scheduler. The caller must determine that
//! the original system would invoke this branch for the entity and timestep.
//! Force-position and move-stop flags are checked here; ability casting,
//! interpolation, collision and world/debug state are the caller's responsibility.

use anyhow::{ensure, Context, Result};
use rrising_ecs::{Entity, TypeIndex, World};

use crate::{
    movement::CharacterMovementFields,
    ordinary_movement::{update_ordinary_movement, OrdinaryMovement},
};

/// Component identities recovered for the pinned build.
#[derive(Clone, Copy, Debug)]
pub struct OrdinaryMovementTypes {
    pub movement: TypeIndex,
    pub velocity: TypeIndex,
    pub move_velocity: TypeIndex,
    pub translation: TypeIndex,
}

impl OrdinaryMovementTypes {
    pub fn from_world(world: &World) -> Result<Self> {
        let lookup = |name: &str| {
            world
                .types
                .type_index_of(name)
                .with_context(|| format!("missing {name}"))
        };
        Ok(Self {
            movement: lookup("ProjectM.Movement")?,
            velocity: lookup("ProjectM.Velocity")?,
            move_velocity: lookup("ProjectM.MoveVelocity")?,
            translation: lookup("Unity.Transforms.Translation")?,
        })
    }
}

fn floats<const N: usize>(bytes: &[u8]) -> Result<[f32; N]> {
    ensure!(bytes.len() == N * 4, "unexpected component size");
    Ok(std::array::from_fn(|i| {
        f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().expect("checked size"))
    }))
}

/// Apply the recovered arithmetic to one entity after the caller has established
/// ordinary-branch eligibility. No input or action fields are synthesized.
///
/// `can_move` and `delta_time` are explicit because their original calculation
/// and the surrounding update loop have not been reconstructed. This function
/// does not perform collision detection or handle animation or networking.
pub fn step_ordinary_entity(
    world: &mut World,
    entity: Entity,
    types: OrdinaryMovementTypes,
    can_move: bool,
    delta_time: f32,
) -> Result<()> {
    let movement =
        CharacterMovementFields::read(world.store.component_bytes(entity, types.movement)?)?;
    step_ordinary_command(
        world,
        entity,
        types,
        movement.move_input,
        can_move,
        delta_time,
    )
}

/// Apply the same bounded arithmetic using a direction supplied by a command.
/// This corresponds only to the data source at DoCommand's DoMovement call;
/// command creation, eligibility, speed modifiers and the scheduler are not implemented.
pub fn step_ordinary_command(
    world: &mut World,
    entity: Entity,
    types: OrdinaryMovementTypes,
    command_direction: [f32; 2],
    can_move: bool,
    delta_time: f32,
) -> Result<()> {
    let movement_bytes = world.store.component_bytes(entity, types.movement)?;
    let movement = CharacterMovementFields::read(movement_bytes)?;
    ensure!(movement_bytes[60] == 0, "force position is active");
    ensure!(!movement.has_move_stop_data, "move stop is active");
    let velocity = floats::<3>(world.store.component_bytes(entity, types.velocity)?)?;
    let mut move_velocity: [u8; 12] = world
        .store
        .component_bytes(entity, types.move_velocity)?
        .try_into()
        .context("MoveVelocity needs 12 bytes")?;
    let translation = floats::<3>(world.store.component_bytes(entity, types.translation)?)?;
    if can_move && delta_time == 0.0 {
        return Ok(());
    }
    let mut state = OrdinaryMovement {
        velocity,
        move_velocity: f32::from_le_bytes(move_velocity[..4].try_into().expect("checked size")),
        translation,
    };
    update_ordinary_movement(
        &mut state,
        command_direction,
        movement.speed,
        movement.ability_cast_speed_multiplier,
        delta_time,
        can_move,
    );
    let velocity_bytes: Vec<_> = state
        .velocity
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    let translation_bytes: Vec<_> = state
        .translation
        .into_iter()
        .flat_map(f32::to_le_bytes)
        .collect();
    move_velocity[..4].copy_from_slice(&state.move_velocity.to_le_bytes());
    world
        .store
        .set_component_bytes(entity, types.velocity, &velocity_bytes)?;
    world
        .store
        .set_component_bytes(entity, types.move_velocity, &move_velocity)?;
    if can_move {
        world
            .store
            .set_component_bytes(entity, types.translation, &translation_bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rrising_ecs::{SpecialTypes, TypeManager, TypeSpec, WorldInput};

    fn fixture() -> (World, Entity, OrdinaryMovementTypes) {
        let mut types = TypeManager::default();
        types.register(TypeSpec::tag("<reserved>")).unwrap();
        let entity = types
            .register(TypeSpec::component_data("Entity", 8, 8))
            .unwrap();
        let m = types
            .register(TypeSpec::component_data("ProjectM.Movement", 104, 16))
            .unwrap();
        let v = types
            .register(TypeSpec::component_data("ProjectM.Velocity", 12, 16))
            .unwrap();
        let mv = types
            .register(TypeSpec::component_data("ProjectM.MoveVelocity", 12, 16))
            .unwrap();
        let t = types
            .register(TypeSpec::component_data(
                "Unity.Transforms.Translation",
                12,
                16,
            ))
            .unwrap();
        let mut world =
            World::build(types, SpecialTypes::new(entity), 8, &WorldInput::default()).unwrap();
        let arch = world
            .store
            .get_or_create_archetype(&world.types, &[m, v, mv, t])
            .unwrap();
        let entity = world.store.create_entity(arch).unwrap();
        let mut movement = [0u8; 104];
        for (offset, value) in [(76, 0.5f32), (80, -1.0), (92, 4.0), (96, 1.0)] {
            movement[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        world
            .store
            .set_component_bytes(entity, m, &movement)
            .unwrap();
        world
            .store
            .set_component_bytes(entity, mv, &[0, 0, 0, 0, 9, 8, 7, 6, 5, 4, 3, 2])
            .unwrap();
        (
            world,
            entity,
            OrdinaryMovementTypes {
                movement: m,
                velocity: v,
                move_velocity: mv,
                translation: t,
            },
        )
    }

    #[test]
    fn applies_only_the_bounded_branch_and_preserves_other_fields() {
        let (mut world, entity, types) = fixture();
        step_ordinary_entity(&mut world, entity, types, true, 0.25).unwrap();
        let translation = floats::<3>(
            world
                .store
                .component_bytes(entity, types.translation)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(translation, [0.5, 0.0, -1.0]);
        assert_eq!(
            &world
                .store
                .component_bytes(entity, types.move_velocity)
                .unwrap()[4..],
            &[9, 8, 7, 6, 5, 4, 3, 2]
        );
        let before = world
            .store
            .component_bytes(entity, types.translation)
            .unwrap()
            .to_vec();
        let mut movement = world
            .store
            .component_bytes(entity, types.movement)
            .unwrap()
            .to_vec();
        movement[100] = 1;
        world
            .store
            .set_component_bytes(entity, types.movement, &movement)
            .unwrap();
        assert!(step_ordinary_entity(&mut world, entity, types, true, 1.0).is_err());
        assert_eq!(
            world
                .store
                .component_bytes(entity, types.translation)
                .unwrap(),
            before
        );
    }

    #[test]
    fn command_direction_is_distinct_from_movement_component() {
        let (mut world, entity, types) = fixture();
        let before = world
            .store
            .component_bytes(entity, types.movement)
            .unwrap()
            .to_vec();
        step_ordinary_command(&mut world, entity, types, [-1.0, 0.25], true, 0.5).unwrap();
        let position = floats::<3>(
            world
                .store
                .component_bytes(entity, types.translation)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(position, [-2.0, 0.0, 0.5]);
        assert_eq!(
            world.store.component_bytes(entity, types.movement).unwrap(),
            before
        );
    }
}
