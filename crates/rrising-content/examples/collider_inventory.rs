//! Inventory serialized PhysicsCollider references in the owner's local scenes.
//! This counts data; it does not reconstruct Unity Physics or collision behavior.
use anyhow::{Context, Result};
use rrising_content::{
    entity_scenes::SceneFile,
    physics_collider::{read_scene_colliders, PHYSICS_COLLIDER_HASH},
    type_layouts::TypeLayouts,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

fn main() -> Result<()> {
    let data = PathBuf::from(std::env::var_os("VRISING_DATA").context("VRISING_DATA")?);
    let layouts = TypeLayouts::from_json(&std::fs::read(
        std::env::var_os("RRISING_LAYOUT_REPORT").context("RRISING_LAYOUT_REPORT")?,
    )?)?;
    let dir = data.join("StreamingAssets/EntityScenes");
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|x| x == "entities") {
            paths.push(path);
        }
    }
    paths.sort();
    let mut files = 0;
    let mut entities = 0;
    let mut nulls = 0;
    let mut kinds = BTreeMap::<u8, usize>::new();
    let mut unique = BTreeSet::new();
    for path in paths {
        let scene = SceneFile::open(&path)?;
        if !scene
            .archetypes()?
            .unique_type_hashes
            .contains(&PHYSICS_COLLIDER_HASH)
        {
            continue;
        }
        let colliders = read_scene_colliders(&scene, &layouts)
            .with_context(|| format!("scene {}", path.display()))?;
        if colliders.is_empty() {
            continue;
        }
        files += 1;
        entities += colliders.len();
        let mut scene_kinds = BTreeMap::<u8, usize>::new();
        for collider in &colliders {
            if let (Some(hash), Some(header)) = (collider.blob_hash, collider.header) {
                unique.insert(hash);
                *kinds.entry(header.shape).or_default() += 1;
                *scene_kinds.entry(header.shape).or_default() += 1;
            } else {
                nulls += 1;
            }
        }
        println!(
            "scene={} colliders={} shape_bytes={scene_kinds:?}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            colliders.len()
        );
    }
    println!("collider scenes={files} entities={entities} null={nulls} unique_blob_hashes={} shape_bytes={kinds:?}", unique.len());
    Ok(())
}
