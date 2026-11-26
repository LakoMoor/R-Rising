//! Inspect candidate character resources in an owner's installation.
use rrising_content::{list_archives, GameContent};
use std::path::Path;

fn main() -> anyhow::Result<()> {
    let data = std::env::var("VRISING_DATA")?;
    let archives = list_archives(&Path::new(&data).join("StreamingAssets/ContentArchives"))?;
    let content = GameContent::open(
        Path::new(&data),
        &archives,
        Path::new("assets/typetrees.tpk"),
    )?;
    if let Some(file) = std::env::args().nth(1) {
        let scene = unity_formats::scene::Scene::read(content.player(), &file)?;
        let (renderers, skipped) = scene.renderers(content.player())?;
        for renderer in renderers {
            if let Some(skin) = renderer.skin {
                let mesh = content.read_mesh(&renderer.mesh)?;
                let joints = skin
                    .bones
                    .iter()
                    .flatten()
                    .filter(|&&id| scene.joint(id).ok().flatten().is_some())
                    .count();
                let (blend_vertices, invalid) = mesh.skin.as_ref().map_or((0, 0), |data| {
                    (
                        data.channel.vertices(),
                        data.channel
                            .bones
                            .iter()
                            .zip(&data.channel.weights)
                            .filter(|(bone, weight)| {
                                **weight != 0.0 && **bone as usize >= skin.bones.len()
                            })
                            .count(),
                    )
                });
                println!(
                    "path={} mesh={:?} vertices={} bones={} joints={} bind_poses={} blend_vertices={} invalid_influences={} root={:?} off={:?}",
                    renderer.path,
                    mesh.name,
                    mesh.vertex_count,
                    skin.bones.len(),
                    joints,
                    mesh.skin.as_ref().map_or(0, |s| s.bind_poses.len()),
                    blend_vertices,
                    invalid,
                    skin.root_bone,
                    renderer.off
                );
            }
        }
        eprintln!("skipped={skipped:?}");
        return Ok(());
    }
    for entry in content.entries().iter().filter(|e| {
        matches!(e.class.as_str(), "GameObject" | "Mesh")
            && ["player", "vampire"]
                .iter()
                .any(|name| e.name.to_lowercase().contains(name))
    }) {
        println!(
            "{}\t{}\t{}\t{}",
            entry.class, entry.name, entry.object.file, entry.object.path_id
        );
    }
    Ok(())
}
