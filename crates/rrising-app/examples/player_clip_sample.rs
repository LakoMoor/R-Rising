//! Compare sampled original clip poses; no gameplay state-machine evaluation.
use anyhow::{Context, Result};
use rrising_content::{list_archives, GameContent};
use rrising_render::clip_pose::sample_bones;
use std::path::Path;
use unity_formats::{
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
    let animator = scene
        .animators(content.player())?
        .into_iter()
        .find(|a| a.path.starts_with("HYB_VampireMale_Prefab/"))
        .context("animator")?;
    let root = scene
        .transform_of(animator.game_object)
        .context("animator root")?;
    let override_ref = animator.controller.context("override controller")?;
    let override_data = content.player().decode(&override_ref)?;
    let base = content
        .player()
        .resolve(
            &override_ref.file,
            override_data.get("m_Controller").context("base pointer")?,
        )?
        .context("base controller")?;
    let clips = controller_clips(content.player(), &base)?;
    let clip = ClipData::read(content.player(), clips.first().context("no clip")?)?;
    let (renderers, _) = scene.renderers(content.player())?;
    let skin = renderers
        .into_iter()
        .find(|r| r.path.starts_with("HYB_VampireMale_Prefab/") && r.path.ends_with("Body_GEO"))
        .context("body")?
        .skin
        .context("skin")?;
    let bones = skin
        .bones
        .into_iter()
        .map(|v| v.context("missing bone"))
        .collect::<Result<Vec<_>>>()?;
    let mut empty = clip.clone();
    empty.tracks.clear();
    let base_pose = sample_bones(&scene, root, &bones, &empty, 0.0)?;
    for (&bone, sample) in bones.iter().zip(&base_pose) {
        let original = scene.world_of(bone)?;
        anyhow::ensure!(
            sample
                .iter()
                .flatten()
                .zip(original.iter().flatten())
                .all(|(a, b)| (a - b).abs() < 1e-8),
            "rest pose differs from scene"
        );
    }
    let first = sample_bones(&scene, root, &bones, &clip, 0.0)?;
    let second = sample_bones(&scene, root, &bones, &clip, 0.3)?;
    let changed = first
        .iter()
        .zip(&second)
        .filter(|(a, b)| {
            a.iter()
                .flatten()
                .zip(b.iter().flatten())
                .any(|(x, y)| (x - y).abs() > 1e-5)
        })
        .count();
    anyhow::ensure!(changed > 0, "clip did not move any bone");
    println!(
        "{}: {} bones, {changed} changed between t=0 and t=0.3; rest pose matches hierarchy",
        clip.name,
        bones.len()
    );
    Ok(())
}
