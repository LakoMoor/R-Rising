//! Original waypoint selection used by `MoveTowardsPositionSystem_Shared_Update`.
//! The native function at RVA 0x75c6390 is replayed without helper stubs on 4,096
//! cases. This is not a path search, collision solver or complete movement system.
use anyhow::{ensure, Result};

/// Serialized `PathWaypoints` (20 bytes; the final two bytes are padding).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathWaypoints {
    pub waypoint: [i32; 2],
    pub next_waypoint: [i32; 2],
    pub has_waypoint: bool,
    pub has_next_waypoint: bool,
}

impl PathWaypoints {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        ensure!(bytes.len() == 20, "PathWaypoints needs 20 bytes");
        let read = |at| i32::from_le_bytes(bytes[at..at + 4].try_into().expect("checked size"));
        Ok(Self {
            waypoint: [read(0), read(4)],
            next_waypoint: [read(8), read(12)],
            has_waypoint: bytes[16] != 0,
            has_next_waypoint: bytes[17] != 0,
        })
    }
}

fn world_position(point: [i32; 2], radius: f32) -> [f32; 2] {
    // The three constants are read from original RVAs cf29504, cf28a88, cf28704.
    // Keep this operation order: radius is added before the final 0.05 offset.
    point.map(|v| ((v as f32 - 6400.0) * 0.5 + radius) + 0.05)
}

/// `PathfindingUtility.TryGetTargetPositionFromWaypoints` (RVA 0x75c6390).
///
/// When the current waypoint is behind/on the velocity plane, prefer the next
/// waypoint if present. An unordered (NaN) comparison also selects the next point:
/// the original `comiss`/`ja` skips that branch only for a strictly positive dot.
/// With neither point, return false and overwrite the output with positive zeros.
pub fn try_get_target_position_from_waypoints(
    waypoints: &PathWaypoints,
    position: [f32; 2],
    velocity_dir: [f32; 2],
    world_space_radius: f32,
    target_position: &mut [f32; 2],
) -> bool {
    if waypoints.has_waypoint {
        let mut target = world_position(waypoints.waypoint, world_space_radius);
        let dot = (target[1] - position[1]) * velocity_dir[1]
            + (target[0] - position[0]) * velocity_dir[0];
        if (dot <= 0.0 || dot.is_nan()) && waypoints.has_next_waypoint {
            target = world_position(waypoints.next_waypoint, world_space_radius);
        }
        *target_position = target;
        true
    } else if waypoints.has_next_waypoint {
        *target_position = world_position(waypoints.next_waypoint, world_space_radius);
        true
    } else {
        *target_position = [0.0; 2];
        false
    }
}
