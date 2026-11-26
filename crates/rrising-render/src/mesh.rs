//! Turning a stored `Mesh` into GPU buffers.
//!
//! The vertex stream is rebuilt from the mesh's own channels (`Vertex`, `Normal`, `TexCoord0` in
//! Unity's channel order) into the interleaved layout the port's shader reads. Positions, normals
//! and uvs are the values the file stores, in Unity's frame — no axis flip happens here, so a
//! mismatch shows up as a wrong transform, not as silently mangled data.

#[cfg(feature = "unity")]
use anyhow::anyhow;
use anyhow::Result;
#[cfg(feature = "unity")]
use unity_formats::mesh::UnityMeshData;
use wgpu::util::DeviceExt;

/// One vertex as the port's shader expects it: 32 bytes, `bytemuck`-castable.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, serde::Serialize, serde::Deserialize)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

/// A mesh ready to draw.
pub struct GpuMesh {
    pub name: String,
    pub vertex_count: usize,
    pub index_count: usize,
    pub submesh_count: usize,
    pub skinned: bool,
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
}

/// Interleave a stored mesh's channels into `Vertex` rows and collect its indices.
#[cfg(feature = "unity")]
pub fn build_vertices(mesh: &UnityMeshData) -> Result<(Vec<Vertex>, Vec<u32>)> {
    let n = mesh.vertex_count;
    if n == 0 {
        return Err(anyhow!("mesh {:?} has no vertices", mesh.name));
    }
    let positions = mesh.channels[0]
        .as_ref()
        .ok_or_else(|| anyhow!("mesh {:?} has no position channel", mesh.name))?;
    if positions.dim != 3 || positions.values.len() < n * 3 {
        return Err(anyhow!(
            "mesh {:?}: position channel is {} wide with {} values for {} vertices",
            mesh.name,
            positions.dim,
            positions.values.len(),
            n
        ));
    }
    // Normals and uv0 are optional in the files; a mesh without them gets a flat up normal and a
    // zero uv, which the shader then samples at one texel — visible, and honest about the gap.
    let normals = mesh.channels[1].as_ref().filter(|c| c.dim >= 3);
    let uvs = mesh.channels[4].as_ref().filter(|c| c.dim >= 2);

    let mut verts = Vec::with_capacity(n);
    for i in 0..n {
        let p = &positions.values[i * positions.dim..i * positions.dim + 3];
        let normal = match normals {
            Some(c) => [
                c.values[i * c.dim],
                c.values[i * c.dim + 1],
                c.values[i * c.dim + 2],
            ],
            None => [0.0, 1.0, 0.0],
        };
        let uv = match uvs {
            Some(c) => [c.values[i * c.dim], c.values[i * c.dim + 1]],
            None => [0.0, 0.0],
        };
        verts.push(Vertex {
            position: [p[0], p[1], p[2]],
            normal,
            uv,
        });
    }

    let indices = mesh.indices.clone();
    if indices.is_empty() {
        return Err(anyhow!("mesh {:?} has no indices", mesh.name));
    }
    if let Some(max) = indices.iter().max().copied() {
        if usize::try_from(max).unwrap_or(usize::MAX) >= n {
            return Err(anyhow!(
                "mesh {:?}: index {} is past {} vertices",
                mesh.name,
                max,
                n
            ));
        }
    }
    Ok((verts, indices))
}

/// Upload a mesh's vertices and indices.
#[cfg(feature = "unity")]
pub fn upload(device: &wgpu::Device, mesh: &UnityMeshData) -> Result<GpuMesh> {
    let (verts, indices) = build_vertices(mesh)?;
    upload_vertices(
        device,
        &mesh.name,
        &verts,
        &indices,
        mesh.submeshes.len(),
        mesh.skin.is_some(),
    )
}

pub fn upload_vertices(
    device: &wgpu::Device,
    name: &str,
    verts: &[Vertex],
    indices: &[u32],
    submesh_count: usize,
    skinned: bool,
) -> Result<GpuMesh> {
    anyhow::ensure!(
        !verts.is_empty() && !indices.is_empty(),
        "empty mesh {name:?}"
    );
    anyhow::ensure!(
        indices.iter().all(|&i| (i as usize) < verts.len()),
        "mesh {name:?} has out-of-range indices"
    );
    anyhow::ensure!(
        verts.iter().all(|v| v
            .position
            .iter()
            .chain(&v.normal)
            .chain(&v.uv)
            .all(|x| x.is_finite())),
        "mesh {name:?} contains nonfinite coordinates"
    );
    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("verts {name}")),
        contents: bytemuck::cast_slice(verts),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(&format!("inds {name}")),
        contents: bytemuck::cast_slice(indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    Ok(GpuMesh {
        name: name.to_owned(),
        vertex_count: verts.len(),
        index_count: indices.len(),
        submesh_count,
        skinned,
        vertex_buffer,
        index_buffer,
    })
}

/// Replace the vertices of an existing mesh without changing its topology.
pub fn update_vertices(queue: &wgpu::Queue, mesh: &GpuMesh, vertices: &[Vertex]) -> Result<()> {
    anyhow::ensure!(vertices.len() == mesh.vertex_count, "vertex count changed");
    anyhow::ensure!(
        vertices.iter().all(|v| v
            .position
            .iter()
            .chain(&v.normal)
            .chain(&v.uv)
            .all(|x| x.is_finite())),
        "nonfinite vertex"
    );
    queue.write_buffer(&mesh.vertex_buffer, 0, bytemuck::cast_slice(vertices));
    Ok(())
}

/// The vertex layout the pipeline is built with; must match `Vertex` and the shader's inputs.
pub fn vertex_layout() -> [Option<wgpu::VertexBufferLayout<'static>>; 2] {
    const VERTEX: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2];
    // The instance buffer carries the model matrix as four columns, one attribute each.
    const INSTANCE: [wgpu::VertexAttribute; 4] =
        wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Float32x4];
    [
        Some(wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &VERTEX,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: 64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &INSTANCE,
        }),
    ]
}
