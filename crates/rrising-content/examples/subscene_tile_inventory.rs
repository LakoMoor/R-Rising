//! Resolve baked server spawn requests to local client tile definitions where direct.
use anyhow::{Context, Result};
use rrising_content::{
    entity_scenes::SceneFile,
    subscene_spawns::{read_spawns, SpawnKind},
    tile_catalog::TileTemplateCatalog,
    type_layouts::TypeLayouts,
};
use std::path::{Path, PathBuf};
fn scenes(data: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(data.join("StreamingAssets/EntityScenes"))? {
        let p = e?.path();
        if p.extension().is_some_and(|x| x == "entities") {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}
fn main() -> Result<()> {
    let client = PathBuf::from(std::env::var_os("VRISING_DATA").context("VRISING_DATA")?);
    let server =
        PathBuf::from(std::env::var_os("VRISING_SERVER_DATA").context("VRISING_SERVER_DATA")?);
    let layouts = TypeLayouts::from_json(&std::fs::read(
        std::env::var_os("RRISING_LAYOUT_REPORT").context("RRISING_LAYOUT_REPORT")?,
    )?)?;
    let server_layouts = TypeLayouts::from_server_json(&std::fs::read(
        std::env::var_os("RRISING_SERVER_LAYOUT_REPORT").context("RRISING_SERVER_LAYOUT_REPORT")?,
    )?)?;
    let mut catalog = TileTemplateCatalog::default();
    for p in scenes(&client)? {
        catalog.read_scene(&SceneFile::open(&p)?, &layouts)?;
    }
    let mut direct = 0;
    let mut chains = 0;
    let mut matched = 0;
    let mut indexed = 0;
    let mut negative = 0;
    for p in scenes(&server)? {
        for spawn in read_spawns(&SceneFile::open(&p)?, &server_layouts)? {
            match spawn.kind {
                SpawnKind::Direct => {
                    direct += 1;
                    if catalog.get(spawn.guid).is_some() {
                        matched += 1;
                    }
                }
                SpawnKind::Chain { .. } => chains += 1,
            }
            if let Some((_, index)) = spawn.static_index {
                indexed += 1;
                negative += usize::from(index < 0);
            }
        }
    }
    println!("direct={direct} chain={chains} direct_tile_matches={matched} static_indices={indexed} negative_indices={negative}");
    Ok(())
}
