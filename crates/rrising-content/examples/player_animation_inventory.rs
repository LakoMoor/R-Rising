//! Inspect the Animator and its clips on the candidate male gameplay prefab.
//! Requires the owner's installed client; no game assets are written.
use anyhow::{Context, Result};
use rrising_content::{list_archives, GameContent};
use std::path::Path;
use unity_formats::{
    animator::UnityAvatar,
    clip::{controller_clips, ClipData},
    scene::Scene,
};

fn main() -> Result<()> {
    let data = std::env::var("VRISING_DATA")?;
    let archives = list_archives(&Path::new(&data).join("StreamingAssets/ContentArchives"))?;
    let content = GameContent::open(
        Path::new(&data),
        &archives,
        Path::new("assets/typetrees.tpk"),
    )?;
    let scene = Scene::read(content.player(), "ae19a255904fd765632aa97a2d7c670c")?;
    let (renderers, _) = scene.renderers(content.player())?;
    for animator in scene
        .animators(content.player())?
        .into_iter()
        .filter(|a| a.path.starts_with("HYB_VampireMale_Prefab"))
    {
        println!(
            "animator {} controller={:?} avatar={:?}",
            animator.path, animator.controller, animator.avatar
        );
        if let Some(avatar) = animator.avatar {
            match UnityAvatar::read(content.player(), &avatar) {
                Ok(a) => println!(
                    "avatar {:?}: {} nodes, humanoid={}",
                    a.name,
                    a.skeleton.nodes.len(),
                    a.human.is_some()
                ),
                Err(e) => println!("avatar unavailable: {e}"),
            }
        }
        if let Some(controller) = animator.controller {
            println!(
                "controller class {:?}",
                content.player().class_name(&controller)
            );
            let value = content.player().decode(&controller)?;
            println!("controller base {:?}", value.get("m_Controller"));
            let base = match value.get("m_Controller") {
                Some(pointer) => content
                    .player()
                    .resolve(&controller.file, pointer)?
                    .context("null base controller")?,
                None => controller,
            };
            let clips = controller_clips(content.player(), &base)?;
            println!("controller has {} clips", clips.len());
            let root = scene
                .transform_of(animator.game_object)
                .context("animator transform")?;
            let paths: std::collections::BTreeSet<_> = renderers
                .iter()
                .filter(|r| r.path.starts_with("HYB_VampireMale_Prefab/"))
                .filter_map(|r| r.skin.as_ref())
                .flat_map(|s| s.bones.iter().flatten())
                .filter_map(|&id| scene.path_below(id, root))
                .map(|path| unity_formats::clip::path_hash(&path))
                .collect();
            println!("{} unique skinned bone paths under animator", paths.len());
            for reference in clips.iter().take(30) {
                match ClipData::read(content.player(), reference) {
                    Ok(clip) => println!(
                        "  {:?}: {:.3}s, {} tracks, {} target known bones",
                        clip.name,
                        clip.duration,
                        clip.tracks.len(),
                        clip.tracks
                            .iter()
                            .filter(|track| track.binding.type_id == 4
                                && paths.contains(&track.binding.path))
                            .count()
                    ),
                    Err(e) => println!("  {reference:?}: {e}"),
                }
            }
        }
    }
    Ok(())
}
