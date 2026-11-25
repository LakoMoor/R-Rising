//! `rrising-core` — the ported gameplay layer.
//!
//! The plan is to translate `ProjectM.*` and `Stunlock.*` here, method by method, from the
//! decompiled IL2CPP build. What exists today is the content-facing half of that layer: assembling
//! drawable scenes from the original's archives. The gameplay systems themselves wait on the
//! decompiled bodies, which `docs/PORT.md` tracks.

pub mod aim;
pub mod easing;
pub mod helpers;
pub mod movement;
pub mod pathfinding;
#[cfg(feature = "content")]
pub mod scene;

#[cfg(feature = "content")]
pub use scene::{base_map_name, showcase, SceneItem, SceneSpec};
