//! `ProjectM.AimPrediction` — first-order intercept used for projectile aiming.
//!
//! src: original client RVAs 0x7f8f010 (`FirstOrderInterceptTime`) and 0x7f8ef70
//! (`FirstOrderIntercept`). The branch structure and float operation order follow the original
//! instructions; `math.min/max` use the Unity.Mathematics NaN rule observed in them
//! (`IsNaN(y) || x OP y ? x : y`). Verified against `evidence/oracles/aim_prediction.tsv`.

const EPSILON: f32 = 0.001;

fn max(x: f32, y: f32) -> f32 {
    if y.is_nan() || x > y {
        x
    } else {
        y
    }
}

fn min(x: f32, y: f32) -> f32 {
    if y.is_nan() || x < y {
        x
    } else {
        y
    }
}

/// Time at which a shot of `shot_speed` meets a target moving with `rel_vel` from `rel_pos`.
pub fn first_order_intercept_time(shot_speed: f32, rel_pos: [f32; 2], rel_vel: [f32; 2]) -> f32 {
    let (px, py) = (rel_pos[0], rel_pos[1]);
    let (vx, vy) = (rel_vel[0], rel_vel[1]);
    let velocity_squared = vy * vy + vx * vx;
    if EPSILON > velocity_squared {
        return 0.0;
    }
    let a = velocity_squared - shot_speed * shot_speed;
    let b = 2.0 * (py * vy + px * vx);
    let c = py * py + px * px;
    if EPSILON > a.abs() {
        // The original does not negate here (unlike the common textbook form): a closing target
        // gives a negative quotient that max() clamps to zero.
        let d = py * vy + px * vx;
        return max(c / (d + d), 0.0);
    }
    let determinant = b * b - c * (a * 4.0);
    if determinant > 0.0 {
        let root = determinant.sqrt();
        let t1 = (-b + root) / (a + a);
        let t2 = (-b - root) / (a + a);
        if t1 > 0.0 {
            if t2 > 0.0 {
                min(t1, t2)
            } else {
                t1
            }
        } else {
            max(t2, 0.0)
        }
    } else if 0.0 > determinant {
        0.0
    } else {
        max(-b / (a + a), 0.0)
    }
}

/// Predicted impact point for a moving shooter and target.
pub fn first_order_intercept(
    shooter_position: [f32; 2],
    shooter_velocity: [f32; 2],
    shot_speed: f32,
    target_position: [f32; 2],
    target_velocity: [f32; 2],
) -> [f32; 2] {
    let rel_pos = [
        target_position[0] - shooter_position[0],
        target_position[1] - shooter_position[1],
    ];
    let rel_vel = [
        target_velocity[0] - shooter_velocity[0],
        target_velocity[1] - shooter_velocity[1],
    ];
    let t = first_order_intercept_time(shot_speed, rel_pos, rel_vel);
    [
        rel_vel[0] * t + target_position[0],
        rel_vel[1] * t + target_position[1],
    ]
}
