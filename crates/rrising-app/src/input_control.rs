//! Diagnostic keyboard boundary for the experimental player driver.
use anyhow::Result;
use rrising_ecs::{Entity, TypeIndex, World};
use std::collections::HashSet;
use winit::{event::ElementState, keyboard::KeyCode};

pub fn record_key(keys: &mut HashSet<KeyCode>, code: KeyCode, state: ElementState) {
    if state.is_pressed() {
        keys.insert(code);
    } else {
        keys.remove(&code);
    }
}

pub fn write_input(
    world: &mut World,
    entity: Entity,
    ty: TypeIndex,
    keys: &HashSet<KeyCode>,
    yaw: f32,
    last_direction: &mut [f32; 3],
) -> Result<()> {
    let key = |code| f32::from(u8::from(keys.contains(&code)));
    let mut input = rrising_core::player_movement::MovementInput {
        axis: [
            key(KeyCode::KeyD) - key(KeyCode::KeyA),
            key(KeyCode::KeyW) - key(KeyCode::KeyS),
        ],
        mode: 0,
        slow: false,
        last_direction: *last_direction,
    };
    let movement = rrising_core::player_movement::get_movement(
        &mut input,
        [0.0, (yaw * 0.5).sin(), 0.0, (yaw * 0.5).cos()],
    );
    *last_direction = input.last_direction;
    rrising_core::player_input::write_player_movement(world, entity, ty, [movement[0], movement[2]])
}
