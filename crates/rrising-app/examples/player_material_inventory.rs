//! Read-only inventory of the candidate player and face renderers and materials.
use anyhow::Result;
use rrising_content::{list_archives, GameContent};
use std::path::Path;
use unity_formats::scene::Scene;

fn main() -> Result<()> {
    let data = std::env::var("VRISING_DATA")?;
    let archives = list_archives(&Path::new(&data).join("StreamingAssets/ContentArchives"))?;
    let content = GameContent::open(
        Path::new(&data),
        &archives,
        Path::new("assets/typetrees.tpk"),
    )?;
    for (file, prefix) in [
        (
            "ae19a255904fd765632aa97a2d7c670c",
            "HYB_VampireMale_Prefab/",
        ),
        (
            "ae526d3c19a62d5cad7d4b511d1752c3",
            "VampireMale_Face_Standard_Prefab/",
        ),
    ] {
        let scene = Scene::read(content.player(), file)?;
        let (renderers, skipped) = scene.renderers(content.player())?;
        println!("scene {file} skipped={skipped:?}");
        for renderer in renderers.iter().filter(|r| r.path.starts_with(prefix)) {
            let mesh = content.read_mesh(&renderer.mesh)?;
            println!(
                "{} mesh={} vertices={} submeshes={} skin={} world={:?}",
                renderer.path,
                mesh.name,
                mesh.vertex_count,
                mesh.submeshes.len(),
                renderer.skin.is_some(),
                renderer.world
            );
            for reference in renderer.materials.iter().flatten() {
                match content.read_material(reference) {
                    Ok(material) => {
                        println!(
                            "  material {} base={:?}",
                            material.name,
                            rrising_core::base_map_name(&material)
                        );
                        for (name, env) in material.textures {
                            println!(
                                "    {name}: {:?} scale={:?} offset={:?}",
                                env.texture, env.scale, env.offset
                            );
                        }
                    }
                    Err(error) => println!("  material error: {error:#}"),
                }
            }
        }
    }
    Ok(())
}
