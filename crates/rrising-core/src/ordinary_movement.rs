//! Collision-free, non-casting branch of the server's CommandUtility.DoMovement.
//! Does not implement force-position, move-stop, casting exceptions, or debug code.

/// State modified by the bounded ordinary-movement branch.
#[derive(Clone, Copy, Debug)]
pub struct OrdinaryMovement {
    pub velocity: [f32; 3],
    pub move_velocity: f32,
    pub translation: [f32; 3],
}

/// Apply one step when force-position, casting, and move-stop are all inactive.
pub fn update_ordinary_movement(
    state: &mut OrdinaryMovement,
    input: [f32; 2],
    speed: f32,
    ability_multiplier: f32,
    delta_time: f32,
    can_move: bool,
) {
    if !can_move {
        state.velocity = [0.0; 3];
        state.move_velocity = 0.0;
        return;
    }
    if delta_time == 0.0 {
        return;
    }
    // COMISS branches also replace NaN with the upper bound.
    let multiplier = if ability_multiplier.is_nan() || ability_multiplier > 1.0 {
        1.0
    } else if ability_multiplier < 0.0 {
        0.0
    } else {
        ability_multiplier
    };
    let scale = multiplier * speed;
    let x = scale * input[0];
    let y = scale * 0.0;
    let z = scale * input[1];
    state.velocity = [x, y, z];
    state.move_velocity = ((y * y + x * x) + z * z).sqrt();
    for (p, v) in state.translation.iter_mut().zip(state.velocity) {
        *p += v * delta_time;
    }
}
