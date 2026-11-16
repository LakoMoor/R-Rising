//! A scene transfer must preserve the direct GPU path and reject broken mesh indices.
use rrising_render::{
    mesh,
    snapshot::{Snapshot, SnapshotItem},
    DrawCall, FrameUniforms, Instance, Renderer, Vertex,
};

fn fixture() -> Snapshot {
    let identity = [
        [1., 0., 0., 0.],
        [0., 1., 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    Snapshot {
        version: 1,
        description: "synthetic triangle, no game assets".into(),
        uniforms: FrameUniforms {
            view_proj: identity,
            camera_pos: [0., 0., -1., 0.],
            light_dir: [0., 0., 1., 1.],
            ambient: [0.; 4],
        },
        items: vec![SnapshotItem {
            name: "triangle".into(),
            indices: vec![0, 1, 2],
            transform: Instance { model: identity },
            texture: None,
            vertices: [[-0.9, -0.9, 0.5], [0.9, -0.9, 0.5], [0., 0.9, 0.5]]
                .map(|position| Vertex {
                    position,
                    normal: [0., 0., 1.],
                    uv: [0.; 2],
                })
                .to_vec(),
        }],
    }
}
#[test]
fn serialized_transfer_matches_direct_gpu_upload() {
    let mut renderer = pollster::block_on(Renderer::new_offscreen(64, 64)).expect("GPU");
    let original = fixture();
    let item = &original.items[0];
    let mesh = mesh::upload_vertices(
        renderer.device(),
        &item.name,
        &item.vertices,
        &item.indices,
        1,
        false,
    )
    .expect("mesh");
    let neutral = renderer.neutral_view();
    let direct = renderer
        .render_to_image(
            &original.uniforms,
            &[DrawCall {
                mesh: &mesh,
                instances: &[item.transform],
                texture: &neutral,
            }],
        )
        .expect("direct frame");
    let text = serde_json::to_string(&original).expect("serialize");
    let copy: Snapshot = serde_json::from_str(&text).expect("deserialize");
    let loaded = copy.upload(&renderer).expect("scene upload");
    let transferred = loaded
        .render_to_image(&mut renderer)
        .expect("transferred frame");
    assert_eq!(direct, transferred);
    let mut invalid = fixture();
    invalid.items[0].indices[2] = 100;
    assert!(invalid.upload(&renderer).is_err());
    let mut invalid = fixture();
    invalid.version = 2;
    assert!(invalid.upload(&renderer).is_err());
}
