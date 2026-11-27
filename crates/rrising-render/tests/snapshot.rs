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
        textures: vec![],
        diagnostic_overlay: None,
        diagnostic_player: None,
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
            instances: vec![],
            texture_index: None,
            texture: None,
            cutout: false,
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
                cutout: false,
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

#[test]
fn updated_vertices_change_the_rendered_frame() {
    let mut renderer = pollster::block_on(Renderer::new_offscreen(64, 64)).expect("GPU");
    let scene = fixture();
    let item = &scene.items[0];
    let mesh = mesh::upload_vertices(
        renderer.device(),
        &item.name,
        &item.vertices,
        &item.indices,
        1,
        true,
    )
    .unwrap();
    let neutral = renderer.neutral_view();
    let render = |renderer: &mut Renderer| {
        renderer
            .render_to_image(
                &scene.uniforms,
                &[DrawCall {
                    mesh: &mesh,
                    instances: &[item.transform],
                    texture: &neutral,
                    cutout: false,
                }],
            )
            .unwrap()
            .2
    };
    let first = render(&mut renderer);
    let mut moved = item.vertices.clone();
    for vertex in &mut moved {
        vertex.position[0] += 0.7;
    }
    mesh::update_vertices(renderer.queue(), &mesh, &moved).unwrap();
    let second = render(&mut renderer);
    assert_ne!(first, second);
    assert!(mesh::update_vertices(renderer.queue(), &mesh, &moved[..2]).is_err());
}

#[test]
fn transfer_keeps_optional_overlay_and_rejects_invalid_layout() {
    use rrising_render::{
        overlay::ActionBarLayout, snapshot::SnapshotOverlay, texture::TextureData,
    };
    let mut renderer = pollster::block_on(Renderer::new_offscreen(64, 64)).unwrap();
    let baseline = fixture()
        .upload(&renderer)
        .unwrap()
        .render_to_image(&mut renderer)
        .unwrap()
        .2;
    let mut scene = fixture();
    scene.diagnostic_overlay = Some(SnapshotOverlay {
        texture: TextureData::flat("synthetic", [255, 0, 0, 255]),
        layout: ActionBarLayout {
            slot_size: [300., 300.],
            frame_extra: [0., 0.],
            anchor_position: [0., 600.],
            spacing: 0.,
            count: 1,
        },
    });
    let json = serde_json::to_string(&scene).unwrap();
    let decoded: Snapshot = serde_json::from_str(&json).unwrap();
    let overlay = decoded
        .upload(&renderer)
        .unwrap()
        .render_to_image(&mut renderer)
        .unwrap()
        .2;
    assert_ne!(baseline, overlay);
    let mut invalid = fixture();
    invalid.diagnostic_overlay = Some(SnapshotOverlay {
        texture: TextureData::flat("synthetic", [255, 255, 255, 255]),
        layout: ActionBarLayout {
            slot_size: [48., 48.],
            frame_extra: [0., 0.],
            anchor_position: [0., 0.],
            spacing: 0.,
            count: 65,
        },
    });
    assert!(invalid.upload(&renderer).is_err());
    // Old version-1 files had no diagnostic_overlay key.
    let legacy: Snapshot =
        serde_json::from_str(&serde_json::to_string(&fixture()).unwrap()).unwrap();
    assert!(legacy.diagnostic_overlay.is_none());
}

#[test]
fn player_visual_and_camera_move_together_and_invalid_indices_are_rejected() {
    let mut renderer = pollster::block_on(Renderer::new_offscreen(64, 64)).unwrap();
    let mut loaded = fixture().upload(&renderer).unwrap();
    let base = loaded.uniforms;
    let before = loaded.render_to_image(&mut renderer).unwrap().2;
    assert!(loaded
        .set_diagnostic_player_position(&[0, 0], [0.; 3], [1., 0., 0.], base)
        .is_err());
    assert!(loaded
        .set_diagnostic_player_position(&[0, 12], [0.; 3], [1., 0., 0.], base)
        .is_err());
    assert_eq!(before, loaded.render_to_image(&mut renderer).unwrap().2);
    loaded
        .set_diagnostic_player_position(&[0], [0.; 3], [1., 0., 0.], base)
        .unwrap();
    assert_eq!(loaded.uniforms.view_proj[3][0], -1.0);
    assert_eq!(loaded.uniforms.camera_pos[0], 1.0);
    loaded
        .set_diagnostic_player_position(&[0], [0.; 3], [0.; 3], base)
        .unwrap();
    assert_eq!(before, loaded.render_to_image(&mut renderer).unwrap().2);
}

#[test]
fn transferred_player_animation_updates_mesh_and_restores_pose() {
    use rrising_render::snapshot::{SnapshotAnimationPart, SnapshotPlayerAnimation};
    let mut renderer = pollster::block_on(Renderer::new_offscreen(64, 64)).unwrap();
    let original = fixture();
    let rest = original.items[0].vertices.clone();
    let mut shifted = rest.clone();
    for vertex in &mut shifted {
        vertex.position[0] += 0.5;
    }
    let animation = SnapshotPlayerAnimation {
        duration: 1.0,
        parts: vec![SnapshotAnimationPart {
            rest,
            frames: vec![shifted],
        }],
    };
    let loaded = original.upload(&renderer).unwrap();
    assert_eq!(
        loaded.validate_player_animation(&[0], &animation).unwrap(),
        1
    );
    let before = loaded.render_to_image(&mut renderer).unwrap().2;
    loaded
        .set_player_animation(&renderer, &[0], &animation, Some(0))
        .unwrap();
    let walking = loaded.render_to_image(&mut renderer).unwrap().2;
    assert_ne!(before, walking);
    assert!(loaded
        .set_player_animation(&renderer, &[0], &animation, Some(1))
        .is_err());
    loaded
        .set_player_animation(&renderer, &[0], &animation, None)
        .unwrap();
    assert_eq!(before, loaded.render_to_image(&mut renderer).unwrap().2);
}
