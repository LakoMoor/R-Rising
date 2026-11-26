//! Match pinned server terrain scene GUIDs to locally installed client scene filenames.
//! This inventories identifiers; it does not claim runtime streaming or spawn parity.
use anyhow::{Context, Result};
use rrising_content::{static_transforms::StaticTransformCatalog, type_layouts::TypeLayouts};
use rrising_ecs::Hash128;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

fn main() -> Result<()> {
    let server =
        PathBuf::from(std::env::var_os("VRISING_SERVER_DATA").context("VRISING_SERVER_DATA")?);
    let client = PathBuf::from(std::env::var_os("VRISING_DATA").context("VRISING_DATA")?);
    let layouts = TypeLayouts::from_server_json(&std::fs::read(
        std::env::var_os("RRISING_SERVER_LAYOUT_REPORT").context("RRISING_SERVER_LAYOUT_REPORT")?,
    )?)?;
    let catalog = StaticTransformCatalog::read_server(&server, &layouts)?;
    let mut scenes: BTreeMap<Hash128, Vec<String>> = BTreeMap::new();
    for entry in std::fs::read_dir(client.join("StreamingAssets/EntityScenes"))? {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "entities") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .context("invalid filename")?;
        let hash = Hash128::from_guid(name.split('.').next().context("missing guid")?);
        if hash.is_valid() {
            scenes.entry(hash).or_default().push(name.to_owned());
        }
    }
    let mut found = 0;
    let mut unique = BTreeSet::new();
    let mut matches = BTreeSet::new();
    for coordinate in 0..=u16::MAX {
        let Some(source) = catalog.source(coordinate) else {
            continue;
        };
        unique.insert(source.scene);
        if let Some(files) = scenes.get(&source.scene) {
            found += 1;
            if matches.insert(source.scene) && matches.len() <= 10 {
                println!(
                    "chunk={coordinate:#06x} rotation={} client_files={files:?}",
                    source.rotation_degrees
                );
            }
        }
    }
    println!("server coordinates={} unique source GUIDs={} matching client coordinates={} unique matching GUIDs={} client GUIDs={}", catalog.chunk_count(), unique.len(), found, matches.len(), scenes.len());
    Ok(())
}
