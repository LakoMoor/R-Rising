//! Experimental direct transform-track sampling, without Animator state or blending.
use anyhow::{bail, ensure, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use unity_formats::{
    clip::{path_hash, ClipData},
    scene::{mul, trs, Matrix, Scene},
};

/// Sample transform tracks on the ancestors of `bones` below the Animator root.
/// Output is world-space in bone order. Unsupported Euler/property tracks are ignored.
/// This does not evaluate controller overrides, transitions, root motion or blend shapes.
pub fn sample_bones(
    scene: &Scene,
    root: i64,
    bones: &[i64],
    clip: &ClipData,
    time: f32,
) -> Result<Vec<Matrix>> {
    ensure!(time.is_finite(), "non-finite animation time");
    let mut nodes = BTreeMap::new();
    let mut hashes = BTreeMap::new();
    for &bone in bones {
        let mut current = bone;
        let mut seen = BTreeSet::new();
        loop {
            ensure!(seen.insert(current), "joint hierarchy cycle");
            let path = scene
                .path_below(current, root)
                .context("bone is outside animator root")?;
            let hash = path_hash(&path);
            if let Some(previous) = hashes.insert(hash, current) {
                ensure!(previous == current, "ambiguous animation path hash");
            }
            let joint = scene.joint(current)?.context("missing joint")?;
            let parent = joint.father;
            nodes.insert(current, joint);
            if current == root {
                break;
            }
            current = parent.context("joint chain does not reach animator")?;
        }
    }
    let t = if clip.looping && clip.duration > 0.0 {
        time.rem_euclid(clip.duration)
    } else {
        time.clamp(0.0, clip.duration.max(0.0))
    };
    for track in &clip.tracks {
        if track.binding.type_id != 4 || track.binding.is_pptr {
            continue;
        }
        let Some(id) = hashes.get(&track.binding.path) else {
            continue;
        };
        let joint = nodes.get_mut(id).context("missing mapped joint")?;
        let n = match track.binding.attribute {
            1 | 3 => 3,
            2 => 4,
            _ => continue,
        };
        ensure!(track.curves.len() == n, "invalid transform track width");
        let values: Vec<_> = track
            .curves
            .iter()
            .map(|curve| f64::from(curve.sample(t).0))
            .collect();
        ensure!(
            values.iter().all(|x| x.is_finite()),
            "non-finite sampled transform"
        );
        match track.binding.attribute {
            1 => joint.translation.copy_from_slice(&values),
            2 => {
                let length = values.iter().map(|x| x * x).sum::<f64>().sqrt();
                ensure!(
                    length > 1e-12 && length.is_finite(),
                    "invalid sampled quaternion"
                );
                for (q, v) in joint.rotation.iter_mut().zip(values) {
                    *q = v / length;
                }
            }
            3 => joint.scale.copy_from_slice(&values),
            _ => bail!("unsupported track"),
        }
    }
    let parent_world = match nodes.get(&root).context("missing root")?.father {
        Some(id) => scene.world_of(id)?,
        None => unity_formats::scene::IDENTITY,
    };
    bones
        .iter()
        .map(|&bone| {
            let mut chain = Vec::new();
            let mut current = bone;
            loop {
                let joint = nodes.get(&current).context("missing joint")?;
                chain.push(joint);
                if current == root {
                    break;
                }
                current = joint.father.context("missing parent")?;
            }
            Ok(chain.iter().rev().fold(parent_world, |m, joint| {
                mul(&m, &trs(joint.translation, joint.rotation, joint.scale))
            }))
        })
        .collect()
}
