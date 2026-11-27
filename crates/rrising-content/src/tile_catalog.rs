//! Local TileData definitions indexed by their original PrefabGUID.
use crate::{
    entity_scenes::SceneFile,
    tile_blob::{DenseCollisionTiles, TILE_DATA_HASH},
    type_layouts::TypeLayouts,
};
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;
const GUID: u64 = 0xb181_3197_8820_0ae1;

pub struct TileTemplate {
    pub blob_hash: u64,
    blob: Vec<u8>,
}
impl TileTemplate {
    pub fn collision_tiles(&self) -> Result<DenseCollisionTiles<'_>> {
        DenseCollisionTiles::read(&self.blob)
    }
}
#[derive(Default)]
pub struct TileTemplateCatalog {
    templates: BTreeMap<i32, TileTemplate>,
}
impl TileTemplateCatalog {
    pub fn len(&self) -> usize {
        self.templates.len()
    }
    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }
    pub fn get(&self, guid: i32) -> Option<&TileTemplate> {
        self.templates.get(&guid)
    }
    /// Collect definitions without creating live entities. A conflicting GUID is rejected.
    pub fn read_scene(&mut self, scene: &SceneFile, layouts: &TypeLayouts) -> Result<()> {
        if !scene
            .archetypes()?
            .unique_type_hashes
            .contains(&TILE_DATA_HASH)
        {
            return Ok(());
        }
        let resolved = layouts.resolve_scene(scene)?;
        let blobs = scene.blob_assets()?;
        let mut pending = BTreeMap::<i32, TileTemplate>::new();
        for chunk in &resolved.chunks {
            for row in 0..chunk.header.count as usize {
                let Some(value) = resolved.component_bytes(chunk.index, row, TILE_DATA_HASH)?
                else {
                    continue;
                };
                ensure!(value.len() == 8, "invalid TileData reference size");
                let offset = i32::from_le_bytes(value[..4].try_into()?);
                if offset == -1 {
                    continue;
                }
                let guid = resolved
                    .component_bytes(chunk.index, row, GUID)?
                    .context("TileData has no PrefabGUID")?;
                let guid = i32::from_le_bytes(guid.try_into()?);
                let blob = blobs
                    .find_by_offset(offset)
                    .context("invalid TileData blob reference")?;
                DenseCollisionTiles::read(blob.data)?;
                if let Some(old) = pending.get(&guid).or_else(|| self.templates.get(&guid)) {
                    ensure!(
                        old.blob_hash == blob.hash && old.blob == blob.data,
                        "conflicting tile definition for GUID {guid}"
                    );
                } else {
                    pending.insert(
                        guid,
                        TileTemplate {
                            blob_hash: blob.hash,
                            blob: blob.data.to_vec(),
                        },
                    );
                }
            }
        }
        self.templates.extend(pending);
        Ok(())
    }
}
