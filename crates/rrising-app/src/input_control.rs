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

/// Preserve a new movement press until a simulation step consumes it, even if
/// the OS releases the key before the next redraw. This is a diagnostic input
/// policy, not a reconstruction of the original game's event timing.
pub fn record_player_key(
    keys: &mut HashSet<KeyCode>,
    pending: &mut HashSet<KeyCode>,
    code: KeyCode,
    state: ElementState,
) {
    if state.is_pressed()
        && !keys.contains(&code)
        && matches!(
            code,
            KeyCode::KeyW | KeyCode::KeyA | KeyCode::KeyS | KeyCode::KeyD
        )
    {
        pending.insert(code);
    }
    record_key(keys, code, state);
}

pub fn step_keys(keys: &HashSet<KeyCode>, pending: &HashSet<KeyCode>) -> HashSet<KeyCode> {
    keys.union(pending).copied().collect()
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
