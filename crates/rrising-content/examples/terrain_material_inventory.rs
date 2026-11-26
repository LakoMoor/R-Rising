//! Inspect terrain-like material properties from the owner's installed game.
use anyhow::Result;
use rrising_content::{list_archives, GameContent};
use std::path::Path;

fn main() -> Result<()> {
    let data = std::env::var("VRISING_DATA")?;
    let archives = list_archives(&Path::new(&data).join("StreamingAssets/ContentArchives"))?;
    let content = GameContent::open(
        Path::new(&data),
        &archives,
        Path::new("assets/typetrees.tpk"),
    )?;
    for entry in content
        .entries()
        .iter()
        .filter(|e| e.class == "Material" && e.name.to_lowercase().contains("terrain"))
    {
        let material = content.read_material(&entry.object)?;
        println!(
            "{} {:?} shader={:?}",
            material.name, entry.object, material.shader
        );
        for (name, tex) in material.textures.iter().filter(|(name, _)| {
            name.contains("Base") || name.contains("Mask") || name.contains("Splat")
        }) {
            println!(
                "  {name}: {:?} scale={:?} offset={:?}",
                tex.texture, tex.scale, tex.offset
            );
        }
        for (name, value) in material.floats.iter().filter(|(name, _)| {
            name.contains("Layer") || name.contains("Tiling") || name.contains("UV")
        }) {
            println!("  {name}: {value}");
        }
    }
    Ok(())
}
