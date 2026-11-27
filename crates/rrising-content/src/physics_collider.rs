//! Read serialized Unity Physics collider references and their common headers.
//! This does not implement collider shapes, queries, or ProjectM map collision.
use crate::{entity_scenes::SceneFile, type_layouts::TypeLayouts};
use anyhow::{ensure, Context, Result};

pub const PHYSICS_COLLIDER_HASH: u64 = 0x9936_cd33_eaf5_2908;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColliderHeader {
    pub shape: u8,
    pub collision_type: u8,
    pub version: u8,
    pub magic: u8,
    pub unique_id: u32,
    pub belongs_to: u32,
    pub collides_with: u32,
    pub group_index: i32,
}
impl ColliderHeader {
    /// Common 20-byte header; unknown shape and collision type values remain visible.
    pub fn read(data: &[u8]) -> Result<Self> {
        ensure!(data.len() >= 20, "truncated collider header");
        let word =
            |i: usize| u32::from_le_bytes(data[i..i + 4].try_into().expect("checked length"));
        Ok(Self {
            shape: data[0],
            collision_type: data[1],
            version: data[2],
            magic: data[3],
            unique_id: word(4),
            belongs_to: word(8),
            collides_with: word(12),
            group_index: word(16) as i32,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneCollider {
    pub chunk: usize,
    pub row: usize,
    /// Null serialized references have no blob or header.
    pub blob_hash: Option<u64>,
    pub header: Option<ColliderHeader>,
}

/// Resolve per-entity references against their scene's serialized blob batch.
/// Values are never treated as native pointers and no game data is copied to disk.
pub fn read_scene_colliders(
    scene: &SceneFile,
    layouts: &TypeLayouts,
) -> Result<Vec<SceneCollider>> {
    let resolved = layouts.resolve_scene(scene)?;
    let blobs = scene.blob_assets()?;
    let mut out = Vec::new();
    for chunk in &resolved.chunks {
        let arch = resolved
            .archetypes
            .get(chunk.header.archetype_index as usize)
            .and_then(Option::as_ref)
            .context("unresolved scene archetype")?;
        if !arch.hashes.contains(&PHYSICS_COLLIDER_HASH) {
            continue;
        }
        for row in 0..chunk.header.count as usize {
            let bytes = resolved
                .component_bytes(chunk.index, row, PHYSICS_COLLIDER_HASH)?
                .context("missing collider component")?;
            ensure!(bytes.len() == 8, "invalid collider reference length");
            let offset = i32::from_le_bytes(bytes[..4].try_into().expect("checked length"));
            let blob = if offset == -1 {
                None
            } else {
                Some(
                    blobs
                        .find_by_offset(offset)
                        .context("invalid collider blob offset")?,
                )
            };
            out.push(SceneCollider {
                chunk: chunk.index,
                row,
                blob_hash: blob.map(|b| b.hash),
                header: blob.map(|b| ColliderHeader::read(b.data)).transpose()?,
            });
        }
    }
    Ok(out)
}
