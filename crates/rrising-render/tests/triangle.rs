//! The GPU path on its own, with no content files involved.
//!
//! This isolates the parts that can silently produce an empty frame — the vertex layout, the
//! instance buffer, the uniform block, the depth test and the readback — from the content readers.
//! An identity `view_proj` means the hand-built vertices are already in the port's clip space, so a
//! missing triangle here is a renderer bug, not a transform bug.

use rrising_render::{DrawCall, FrameUniforms, GpuMesh, Instance, Renderer, Vertex};
use wgpu::util::DeviceExt;

const IDENTITY: [[f32; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

#[test]
fn draws_a_hand_built_triangle() {
    let mut renderer =
        pollster::block_on(Renderer::new_offscreen(64, 64)).expect("offscreen device");
    let device = renderer.device();

    let verts = [
        Vertex {
            position: [-0.9, -0.9, 0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0, 0.0],
        },
        Vertex {
            position: [0.9, -0.9, 0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [1.0, 0.0],
        },
        Vertex {
            position: [0.0, 0.9, 0.5],
            normal: [0.0, 0.0, 1.0],
            uv: [0.5, 1.0],
        },
    ];
    let inds: [u32; 3] = [0, 1, 2];
    let mesh = GpuMesh {
        name: "test-triangle".to_string(),
        vertex_count: 3,
        index_count: 3,
        submesh_count: 1,
        skinned: false,
        vertex_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("test verts"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        }),
        index_buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("test inds"),
            contents: bytemuck::cast_slice(&inds),
            usage: wgpu::BufferUsages::INDEX,
        }),
    };

    let instances = [Instance { model: IDENTITY }];
    let uniforms = FrameUniforms {
        view_proj: IDENTITY,
        camera_pos: [0.0, 0.0, -1.0, 0.0],
        // Straight at the triangle, full strength, no ambient: the lit pixels are exactly white.
        light_dir: [0.0, 0.0, 1.0, 1.0],
        ambient: [0.0, 0.0, 0.0, 0.0],
    };

    let neutral = renderer.neutral_view();
    let draws = [DrawCall {
        mesh: &mesh,
        instances: &instances,
        texture: &neutral,
        cutout: false,
    }];
    let (w, h, rgba) = renderer
        .render_to_image(&uniforms, &draws)
        .expect("render offscreen frame");
    assert_eq!((w, h), (64, 64));
    assert_eq!(rgba.len(), (w * h * 4) as usize);

    let clear = (rgba[0], rgba[1], rgba[2]);
    let lit = rgba
        .chunks(4)
        .filter(|p| p[0] > 200 && p[1] > 200 && p[2] > 200)
        .count();
    let untouched = rgba
        .chunks(4)
        .filter(|p| (p[0], p[1], p[2]) == clear)
        .count();

    let mut drawn: Vec<(u8, u8, u8)> = rgba
        .chunks(4)
        .filter(|p| (p[0], p[1], p[2]) != clear)
        .map(|p| (p[0], p[1], p[2]))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    drawn.sort();
    println!("drawn colours: {drawn:?}");

    // The triangle covers about a third of a 64x64 frame; anything far short of that means the
    // rasterizer never saw the draw.
    assert!(
        lit > 800,
        "nothing rasterized: clear {clear:?}, lit {lit}, untouched {untouched} of {}",
        w * h
    );
    assert!(
        untouched > 800,
        "the clear colour is missing too, so the readback is wrong: lit {lit}"
    );
}
