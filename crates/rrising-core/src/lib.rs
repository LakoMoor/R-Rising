//! `rrising-core` — the ported gameplay layer.
//!
//! The plan is to translate `ProjectM.*` and `Stunlock.*` here, method by method, from the
//! decompiled IL2CPP build. What exists today is the content-facing half of that layer: assembling
//! drawable scenes from the original's archives. The gameplay systems themselves wait on the
//! decompiled bodies, which `docs/PORT.md` tracks.

pub mod ability_movement_ecs;
pub mod aim;
pub mod command_movement;
pub mod diagnostic_player;
pub mod easing;
pub mod helpers;
pub mod move_facing;
pub mod movement;
pub mod ordinary_movement;
pub mod ordinary_movement_ecs;
pub mod pathfinding;
pub mod player_input;
pub mod player_movement;
#[cfg(feature = "content")]
pub mod scene;

#[cfg(feature = "content")]
pub use scene::{base_map_name, showcase, SceneItem, SceneSpec};

pub mod tile_position;

pub mod weighted_spawn;

pub mod auto_chain;
#[cfg(feature = "content")]
pub mod auto_chain_ecs;
