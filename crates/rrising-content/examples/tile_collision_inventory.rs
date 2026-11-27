//! Inventory dense map-collision arrays from the owner's local TileData blobs.
use anyhow::{Context, Result};
use rrising_content::{
    entity_scenes::SceneFile,
    tile_blob::{DenseCollisionTiles, TILE_DATA_HASH},
    type_layouts::TypeLayouts,
};
use std::{collections::BTreeSet, path::PathBuf};
fn main() -> Result<()> {
    let data = PathBuf::from(std::env::var_os("VRISING_DATA").context("VRISING_DATA")?);
    let layouts = TypeLayouts::from_json(&std::fs::read(
        std::env::var_os("RRISING_LAYOUT_REPORT").context("RRISING_LAYOUT_REPORT")?,
    )?)?;
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(data.join("StreamingAssets/EntityScenes"))? {
        let path = entry?.path();
        if path.extension().is_some_and(|x| x == "entities") {
            paths.push(path);
        }
    }
    paths.sort();
    let mut refs = 0;
    let mut nulls = 0;
    let mut unique = BTreeSet::new();
    let mut dense = 0;
    let mut nonzero = 0;
    for path in paths {
        let scene = SceneFile::open(&path)?;
        if !scene
            .archetypes()?
            .unique_type_hashes
            .contains(&TILE_DATA_HASH)
        {
            continue;
        }
        let resolved = layouts.resolve_scene(&scene)?;
        let blobs = scene.blob_assets()?;
        let mut count = 0;
        for chunk in &resolved.chunks {
            for row in 0..chunk.header.count as usize {
                let Some(value) = resolved.component_bytes(chunk.index, row, TILE_DATA_HASH)?
                else {
                    continue;
                };
                refs += 1;
                count += 1;
                let offset = i32::from_le_bytes(value[..4].try_into()?);
                if offset == -1 {
                    nulls += 1;
                    continue;
                }
                let blob = blobs
                    .find_by_offset(offset)
                    .context("invalid TileData reference")?;
                let tiles = DenseCollisionTiles::read(blob.data)
                    .with_context(|| format!("scene {}", path.display()))?;
                if unique.insert(blob.hash) {
                    dense += tiles.flags.len();
                    nonzero += tiles.flags.iter().filter(|&&x| x != 0).count();
                }
                anyhow::ensure!(
                    tiles.flags.len() == tiles.exists.len(),
                    "dense collision flags/existence lengths differ"
                );
            }
        }
        println!(
            "scene={} TileData={count}",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
    }
    println!("references={refs} null={nulls} unique_blobs={} dense_entries={dense} nonzero_flags={nonzero}",unique.len());
    Ok(())
}
