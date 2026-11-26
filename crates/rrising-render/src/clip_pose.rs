//! Experimental direct transform-track sampling, without Animator state or blending.
use anyhow::{bail, ensure, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use unity_formats::{
    clip::{path_hash, ClipData},
    scene::{mul, trs, Matrix, Scene},
};

/// Resolve the time for a directly sampled clip. This is independent of the
/// Animator controller, which can impose its own speed and wrap behaviour.
fn sample_time(time: f32, duration: f32, looping: bool, ping_pong: bool) -> Result<f32> {
    ensure!(time.is_finite(), "non-finite animation time");
    ensure!(
        duration.is_finite() && duration >= 0.0,
        "invalid clip duration"
    );
    if duration == 0.0 {
        return Ok(0.0);
    }
    if ping_pong {
        let cycle = f64::from(duration) * 2.0;
        let phase = f64::from(time).rem_euclid(cycle);
        Ok(if phase > f64::from(duration) {
            (cycle - phase) as f32
        } else {
            phase as f32
        })
    } else if looping {
        Ok(time.rem_euclid(duration))
    } else {
        Ok(time.clamp(0.0, duration))
    }
}

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
    let t = sample_time(time, clip.duration, clip.looping, clip.ping_pong)?;
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

#[cfg(test)]
mod tests {
    use super::sample_time;

    #[test]
    fn direct_clip_wrap_modes_and_invalid_times() {
        assert_eq!(sample_time(2.5, 2.0, false, false).unwrap(), 2.0);
        assert_eq!(sample_time(2.5, 2.0, true, false).unwrap(), 0.5);
        assert_eq!(sample_time(2.5, 2.0, false, true).unwrap(), 1.5);
        assert_eq!(sample_time(-0.5, 2.0, false, true).unwrap(), 0.5);
        assert_eq!(sample_time(9.0, 0.0, true, true).unwrap(), 0.0);
        assert!(sample_time(f32::NAN, 2.0, false, false).is_err());
        assert!(sample_time(0.0, f32::NAN, false, false).is_err());
        assert!(sample_time(0.0, -1.0, false, false).is_err());
    }
}
