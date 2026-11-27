use rrising_render::{overlay::OverlayDraw, texture::TextureData, FrameUniforms, Renderer};

#[test]
fn overlay_is_positioned_and_alpha_blended_over_scene() {
    let mut renderer = pollster::block_on(Renderer::new_offscreen(64, 64)).unwrap();
    let tex = TextureData::flat("half red", [255, 0, 0, 128])
        .upload(
            renderer.device(),
            renderer.queue(),
            renderer.device().features(),
        )
        .unwrap();
    let uniforms = FrameUniforms {
        view_proj: [[0.0; 4]; 4],
        camera_pos: [0.0; 4],
        light_dir: [0.0; 4],
        ambient: [0.0; 4],
    };
    let (_, _, before) = renderer.render_to_image(&uniforms, &[]).unwrap();
    let overlay = [OverlayDraw {
        texture: &tex.view,
        rects: &[[8.0, 10.0, 20.0, 16.0]],
        uv: [0.0, 0.0, 1.0, 1.0],
    }];
    let (_, _, after) = renderer
        .render_to_image_with_overlay(&uniforms, &[], &overlay)
        .unwrap();
    let pixel = |image: &[u8], x: usize, y: usize| -> [u8; 4] {
        image[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4]
            .try_into()
            .unwrap()
    };
    assert_eq!(pixel(&before, 2, 2), pixel(&after, 2, 2));
    let changed = pixel(&after, 12, 12);
    assert!(
        changed[0] > 100 && changed[0] < 240,
        "expected blended red: {changed:?}"
    );
    assert!(changed[1] < changed[0] / 2);
    assert_eq!(pixel(&before, 32, 30), pixel(&after, 32, 30));
}
