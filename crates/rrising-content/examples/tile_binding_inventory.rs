//! Cross-reference local TileData and placed TilePosition rows by PrefabGUID.
use anyhow::{Context, Result};
use rrising_content::{
    entity_scenes::SceneFile, tile_blob::TILE_DATA_HASH, type_layouts::TypeLayouts,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
const GUID: u64 = 0xb181_3197_8820_0ae1;
const POSITION: u64 = 0x7d6d_e2e4_7e18_5049;
fn main() -> Result<()> {
    let data = PathBuf::from(std::env::var_os("VRISING_DATA").context("VRISING_DATA")?);
    let layouts = TypeLayouts::from_json(&std::fs::read(
        std::env::var_os("RRISING_LAYOUT_REPORT").context("RRISING_LAYOUT_REPORT")?,
    )?)?;
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(data.join("StreamingAssets/EntityScenes"))? {
        let p = entry?.path();
        if p.extension().is_some_and(|x| x == "entities") {
            paths.push(p);
        }
    }
    paths.sort();
    let mut templates = BTreeMap::<i32, BTreeSet<u64>>::new();
    let mut positions = Vec::new();
    let mut without_guid = 0;
    for path in paths {
        let scene = SceneFile::open(&path)?;
        let hashes = scene.archetypes()?.unique_type_hashes;
        if !hashes.contains(&TILE_DATA_HASH) && !hashes.contains(&POSITION) {
            continue;
        }
        let resolved = layouts.resolve_scene(&scene)?;
        let blobs = scene.blob_assets()?;
        for chunk in &resolved.chunks {
            for row in 0..chunk.header.count as usize {
                let guid = resolved
                    .component_bytes(chunk.index, row, GUID)?
                    .map(|x| i32::from_le_bytes(x.try_into().expect("GUID size")));
                if let Some(value) = resolved.component_bytes(chunk.index, row, TILE_DATA_HASH)? {
                    let offset = i32::from_le_bytes(value[..4].try_into()?);
                    if offset != -1 {
                        let blob = blobs.find_by_offset(offset).context("TileData blob")?;
                        if let Some(guid) = guid {
                            templates.entry(guid).or_default().insert(blob.hash);
                        } else {
                            without_guid += 1;
                        }
                    }
                }
                if let Some(position) = resolved.component_bytes(chunk.index, row, POSITION)? {
                    let prefab = resolved
                        .component_bytes(chunk.index, row, 0x616a_36e3_c0cf_ef8c)?
                        .is_some();
                    positions.push((
                        path.file_name()
                            .context("filename")?
                            .to_string_lossy()
                            .into_owned(),
                        guid,
                        prefab,
                        position.to_vec(),
                    ));
                }
            }
        }
    }
    let mut scene_counts = BTreeMap::<String, [usize; 4]>::new();
    for (scene, guid, prefab, _) in &positions {
        let counts = scene_counts.entry(scene.clone()).or_default();
        counts[0] += 1;
        counts[1] += usize::from(*prefab);
        counts[2] += usize::from(guid.is_some_and(|g| templates.contains_key(&g)));
        counts[3] += usize::from(guid.is_none());
    }
    for (scene, counts) in &scene_counts {
        println!("scene={scene} positions/prefabs/template_matches/no_guid={counts:?}");
    }
    println!("tile_template_guids={} conflicting_blob_guids={} TileData_without_guid={} position_rows={}",templates.len(),templates.values().filter(|s|s.len()>1).count(),without_guid,positions.len());
    Ok(())
}
