//! Inspect compiled-variant parameter metadata for one terrain material's shader.
//! Requires the owner's pinned client; does not export shader programs.
use anyhow::{Context, Result};
use rrising_content::{list_archives, GameContent};
use std::{collections::BTreeSet, path::Path};
use unity_formats::program::{Segments, ShaderPrograms};

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
    let mut cache = Segments::default();
    let mut inspected = 0usize;
    let mut failures = BTreeSet::new();
    let mut failed = 0usize;
    let mut scale_locations = BTreeSet::new();
    for pass in programs.passes.iter().flatten() {
        for stage in [&pass.vertex, &pass.fragment] {
            for variant in &stage.variants {
                let params = match programs.variant_parameters(&mut cache, 4, stage, variant) {
                    Ok(params) => params,
                    Err(error) => {
                        failed += 1;
                        failures.insert(error.to_string());
                        stage.parameters_of(variant).clone()
                    }
                };
                inspected += 1;
                for cb in &params.constant_buffers {
                    for field in &cb.fields {
                        fields.insert(field.name.clone());
                        if field.name == "_WorldUVScale" {
                            scale_locations.insert(format!(
                                "_WorldUVScale: buffer={} slot={:?} offset={} shape={:?}",
                                cb.name, cb.slot, field.offset, field.shape
                            ));
                        }
                    }
                }
                for texture in &params.textures {
                    textures.insert(texture.name.clone());
                }
            }
        }
    }
    println!(
        "visited {inspected} variants; {failed} used incomplete fallback metadata: {failures:?}"
    );
    for location in scale_locations {
        println!("{location}");
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
