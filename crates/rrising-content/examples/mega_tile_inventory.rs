//! Match local baked MegaStaticPrefabBuffer entries to validated tile definitions.
use anyhow::{Context, Result};
use rrising_content::{
    entity_scenes::SceneFile, tile_catalog::TileTemplateCatalog, type_layouts::TypeLayouts,
};
use std::path::PathBuf;
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
    let mut catalog = TileTemplateCatalog::default();
    for path in &paths {
        catalog.read_scene(&SceneFile::open(path)?, &layouts)?;
    }
    let mut buffers = 0;
    let mut slots = 0;
    let mut matched = 0;
    let mut vacant = 0;
    for path in &paths {
        let scene = SceneFile::open(path)?;
        if !scene
            .archetypes()?
            .unique_type_hashes
            .contains(&0xd5ff_c861_8daf_aa4e)
        {
            continue;
        }
        let resolved = layouts.resolve_scene(&scene)?;
        let data = scene.buffer_data()?;
        for chunk in &resolved.chunks {
            for row in 0..chunk.header.count as usize {
                let Some(entries) =
                    resolved.buffer_payload(&data, chunk.index, row, 0xd5ff_c861_8daf_aa4e, 4)?
                else {
                    continue;
                };
                buffers += 1;
                for bytes in entries.as_chunks::<4>().0 {
                    slots += 1;
                    let guid = i32::from_le_bytes(*bytes);
                    if guid == 0 {
                        vacant += 1;
                    } else if catalog.get(guid).is_some() {
                        matched += 1;
                    }
                }
            }
        }
    }
    println!("baked_prefab_buffers={buffers} slots={slots} vacant={vacant} matching_tile_definitions={matched}");
    Ok(())
}
