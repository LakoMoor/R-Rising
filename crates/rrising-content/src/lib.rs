//! `rrising-content` — readers for the content a shipped V Rising build ships with.
//!
//! The port does not convert the game's art into its own format first. It reads the original files
//! directly: UnityFS bundles, the `SerializedFile` containers inside them (with their stored type
//! trees), the `DOTSBIN!` ECS scene binaries under `StreamingAssets/EntityScenes`, the localization
//! JSON, the game-setting presets and the stripped-shader-variant table.
//!
//! That keeps the art 1:1 with the original install and makes every mismatch a reader bug rather
//! than an export bug.

pub mod content;
pub mod entity_scenes;
pub mod hud;
pub mod localization;
pub mod persistence;
pub mod player_customization;
pub mod prefab_catalog;
pub mod restore_world;
pub mod scene_runtime;
pub mod settings;
pub mod static_transforms;

pub use content::{list_archives, ContentEntry, GameContent, TRACKED_CLASSES};

pub mod catalog;

pub mod type_layouts;

pub mod world_adapter;

pub mod shared_values;

pub mod physics_collider;

pub mod tile_blob;

pub mod tile_catalog;
