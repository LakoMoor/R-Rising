//! CPU skinning of the stored four-influence channels for a static pose.
//!
//! `joint_world` matrices are the current scene transforms in bone-slot order.
//! Output positions are world-space; use an identity model matrix when drawing.
//! This experimental path does not apply blend shapes or animation, and normal
//! transformation assumes rigid bone matrices (nonuniform scales need inverse transpose).

use anyhow::{ensure, Context, Result};
use unity_formats::{
    mesh::UnityMeshData,
    scene::{mul, Matrix},
};

use crate::mesh::{build_vertices, Vertex};

pub fn skin_static_pose(
    mesh: &UnityMeshData,
    joint_world: &[Matrix],
) -> Result<(Vec<Vertex>, Vec<u32>)> {
    let skin = mesh.skin.as_ref().context("mesh has no skin")?;
    ensure!(
        joint_world.len() == skin.bind_poses.len(),
        "bone and bind-pose counts differ"
    );
    ensure!(
        skin.channel.vertices() == mesh.vertex_count,
        "blend vertex count differs from mesh"
    );
    let matrices: Vec<_> = joint_world
        .iter()
        .zip(&skin.bind_poses)
        .map(|(joint, bind)| mul(joint, bind))
        .collect();
    let (mut vertices, indices) = build_vertices(mesh)?;
    for (index, vertex) in vertices.iter_mut().enumerate() {
        let (weights, bones) = skin.channel.vertex(index).context("invalid blend range")?;
        let mut position = [0.0f64; 3];
        let mut normal = [0.0f64; 3];
        let mut total = 0.0f64;
        for (&weight, &bone) in weights.iter().zip(bones) {
            ensure!(
                weight.is_finite() && weight >= 0.0,
                "invalid weight at vertex {index}"
            );
            if weight == 0.0 {
                continue;
            }
            let m = matrices
                .get(bone as usize)
                .context("blend bone out of range")?;
            let weight = f64::from(weight);
            total += weight;
            for row in 0..3 {
                let p = vertex.position;
                let n = vertex.normal;
                position[row] += weight
                    * (m[row][0] * f64::from(p[0])
                        + m[row][1] * f64::from(p[1])
                        + m[row][2] * f64::from(p[2])
                        + m[row][3]);
                normal[row] += weight
                    * (m[row][0] * f64::from(n[0])
                        + m[row][1] * f64::from(n[1])
                        + m[row][2] * f64::from(n[2]));
            }
        }
        ensure!(
            (total - 1.0).abs() < 0.001,
            "blend weights sum to {total} at vertex {index}"
        );
        ensure!(
            position.iter().chain(&normal).all(|v| v.is_finite()),
            "nonfinite skinned vertex {index}"
        );
        vertex.position = position.map(|v| v as f32);
        vertex.normal = normal.map(|v| v as f32);
    }
    Ok((vertices, indices))
}

#[cfg(test)]
mod tests {
    use super::*;
    use unity_formats::{
        mesh::{Blend, Channel, MeshSkin},
        scene::IDENTITY,
    };

    #[test]
    fn blends_joint_times_bind_and_rejects_invalid_slots() {
        let mut channels = std::array::from_fn(|_| None);
        channels[0] = Some(Channel {
            dim: 3,
            values: vec![1.0, 2.0, 3.0],
        });
        channels[1] = Some(Channel {
            dim: 3,
            values: vec![0.0, 1.0, 0.0],
        });
        let mut bind = IDENTITY;
        bind[0][3] = 2.0;
        let mut mesh = UnityMeshData {
            name: "one vertex".into(),
            vertex_count: 1,
            channels,
            skin: Some(MeshSkin {
                channel: Blend {
                    starts: vec![0, 2],
                    weights: vec![0.25, 0.75],
                    bones: vec![0, 1],
                },
                unlimited: None,
                bind_poses: vec![IDENTITY, bind],
            }),
            compressed: false,
            shapes: None,
            indices: vec![0],
            index_size: 4,
            submeshes: vec![],
        };
        let mut second = IDENTITY;
        second[1][3] = 4.0;
        let (out, _) = skin_static_pose(&mesh, &[IDENTITY, second]).unwrap();
        assert_eq!(out[0].position, [2.5, 5.0, 3.0]);
        assert_eq!(out[0].normal, [0.0, 1.0, 0.0]);
        mesh.skin.as_mut().unwrap().channel.bones[1] = 2;
        assert!(skin_static_pose(&mesh, &[IDENTITY, second]).is_err());
    }
}
