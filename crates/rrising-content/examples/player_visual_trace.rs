//! Inspect the original client player prefab's linked ECS component identities.
use anyhow::{Context, Result};
use rrising_content::{
    entity_scenes::SceneFile,
    prefab_catalog::ScenePrefabCatalog,
    type_layouts::{TypeLayouts, ENTITY_HASH},
};
use rrising_ecs::Entity;
use std::path::PathBuf;

fn main() -> Result<()> {
    let data = PathBuf::from(std::env::var_os("VRISING_DATA").context("VRISING_DATA")?);
    let layouts = TypeLayouts::from_json(&std::fs::read(
        std::env::var_os("RRISING_LAYOUT_REPORT").context("RRISING_LAYOUT_REPORT")?,
    )?)?;
    let scene = SceneFile::open(
        &data.join("StreamingAssets/EntityScenes/295c92b7a799a3443b40e9ef192b5727.0.entities"),
    )?;
    let catalog = ScenePrefabCatalog::read(&scene, &layouts)?;
    let prefab = catalog
        .prefabs
        .iter()
        .find(|p| p.prefab_guid == 38526109)
        .context("player prefab")?;
    let resolved = layouts.resolve_scene(&scene)?;
    for (ordinal, member) in prefab.linked_entities.iter().enumerate() {
        for (ci, chunk) in resolved.chunks.iter().enumerate() {
            for row in 0..usize::try_from(chunk.header.count)? {
                let Some(id) = resolved.component_bytes(ci, row, ENTITY_HASH)? else {
                    continue;
                };
                let entity = Entity::new(
                    i32::from_le_bytes(id[..4].try_into()?),
                    i32::from_le_bytes(id[4..8].try_into()?),
                );
                if entity != *member {
                    continue;
                }
                println!("member {ordinal} {entity:?}");
                let arch = resolved.archetypes[usize::try_from(chunk.header.archetype_index)?]
                    .as_ref()
                    .context("archetype")?;
                for hash in &arch.hashes {
                    let l = layouts.get(*hash).context("layout")?;
                    println!(
                        "  {:016x} {} {:?} portable={}",
                        hash, l.name, l.spec.category, l.portable_value
                    );
                    if l.name == "ProjectM.Hybrid.UseHybridModel" {
                        let data = resolved
                            .component_bytes(ci, row, *hash)?
                            .context("model data")?;
                        let offset = i32::from_le_bytes(data[..4].try_into()?);
                        let blobs = scene.blob_assets()?;
                        let blob = blobs.find_by_offset(offset).context("model blob")?;
                        println!(
                            "    blob offset={offset} length={} bytes={:02x?}",
                            blob.data.len(),
                            blob.data
                        );
                    }
                }
            }
        }
    }
    Ok(())
}
