//! Inspect compiled-variant parameter metadata for one terrain material's shader.
//! Requires the owner's pinned client; does not export shader programs.
use anyhow::{Context, Result};
use rrising_content::{list_archives, GameContent};
use std::{collections::BTreeSet, path::Path};
use unity_formats::program::ShaderPrograms;

fn main() -> Result<()> {
    let data = std::env::var("VRISING_DATA")?;
    let archives = list_archives(&Path::new(&data).join("StreamingAssets/ContentArchives"))?;
    let content = GameContent::open(
        Path::new(&data),
        &archives,
        Path::new("assets/typetrees.tpk"),
    )?;
    let entry = content
        .entries()
        .iter()
        .find(|e| e.class == "Material" && e.name == "ElrisSnowWhispy01_Terrain")
        .context("material")?;
    let mat = content.read_material(&entry.object)?;
    let programs = ShaderPrograms::read(content.player(), &mat.shader.context("shader")?)?;
    println!(
        "shader {:?}, platforms {:?}",
        programs.name, programs.platforms
    );
    let mut fields = BTreeSet::new();
    let mut textures = BTreeSet::new();
    for pass in programs.passes.iter().flatten() {
        for stage in [&pass.vertex, &pass.fragment] {
            for variant in &stage.variants {
                let params = stage.parameters_of(variant);
                for cb in &params.constant_buffers {
                    for field in &cb.fields {
                        fields.insert(field.name.clone());
                    }
                }
                for texture in &params.textures {
                    textures.insert(texture.name.clone());
                }
            }
        }
    }
    for field in fields.iter().filter(|n| {
        n.contains("Base")
            || n.contains("UV")
            || n.contains("Tiling")
            || n.contains("Layer")
            || n.contains("Splat")
    }) {
        println!("field {field}");
    }
    for texture in &textures {
        println!("texture {texture}");
    }
    Ok(())
}
