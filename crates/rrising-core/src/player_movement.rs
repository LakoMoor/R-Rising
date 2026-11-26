//! Isolated calculation from `GameplayInputSystem.GetMovement` (client RVA 0x7d907d0).
//!
//! The caller supplies camera rotation and the input state. This does not port the input system,
//! action mapping, locomotion or networking. See the replay's limitations in the oracle header.

/// Input and persistent direction fields observed in the original input state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovementInput {
    pub axis: [f32; 2],
    pub mode: u8,
    pub slow: bool,
    pub last_direction: [f32; 3],
}

/// Compute world-space movement and update the last nonzero direction. Rotation is `[x,y,z,w]`.
/// Original reads axis and mode; it writes the direction field only when input exceeds its deadzone.
pub fn get_movement(input: &mut MovementInput, camera: [f32; 4]) -> [f32; 3] {
    let [x, z] = input.axis;
    let len_sq = (x * x + 0.0) + z * z;
    if 0.0001 > len_sq {
        return [0.0; 3];
    }
    let [qx, qy, qz, qw] = camera;
    // Preserve the scalar operation order of the pinned x86 SSE sequence.
    let tx = (qy * z - 0.0 * qz) * 2.0;
    let ty = (qz * x - qx * z) * 2.0;
    let tz = (0.0 * qx - qy * x) * 2.0;
    let rx = (qy * tz - qz * ty) + (qw * tx + x);
    let ry = (qz * tx - qx * tz) + (qw * ty + 0.0);
    let rz = (qx * ty - qy * tx) + (qw * tz + z);
    let length = ((ry * ry + rx * rx) + rz * rz).sqrt();
    let inv = 1.0 / length;
    let norm = [inv * rx, ry * inv, inv * rz];
    input.last_direction = norm;
    match input.mode {
        0 if input.slow => norm.map(|x| x * 0.25),
        0 => norm,
        1 if 0.0625 > len_sq => norm.map(|x| x * 0.25),
        1 if len_sq > 1.0 => norm,
        _ => [rx, ry, rz],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_modes_and_direction_retention() {
        let camera = [0.0, 0.0, 0.0, 1.0];
        let mut state = MovementInput {
            axis: [0.001, 0.0],
            mode: 0,
            slow: false,
            last_direction: [7.0, 8.0, 9.0],
        };
        assert_eq!(get_movement(&mut state, camera), [0.0; 3]);
        assert_eq!(state.last_direction, [7.0, 8.0, 9.0]);
        state.axis = [0.1, 0.0];
        state.slow = true;
        assert_eq!(get_movement(&mut state, camera), [0.25, 0.0, 0.0]);
        assert_eq!(state.last_direction, [1.0, 0.0, 0.0]);
        state.mode = 2;
        assert_eq!(get_movement(&mut state, camera), [0.1, 0.0, 0.0]);
    }
}
