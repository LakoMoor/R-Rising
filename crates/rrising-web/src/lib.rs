//! Browser host for the shared renderer and experimental player projection.
#[cfg(target_arch = "wasm32")]
mod browser {
    use rrising_render::{
        snapshot::{LoadedSnapshot, Snapshot, SnapshotPlayerAnimation},
        FrameUniforms, Renderer,
    };
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub struct BrowserViewer {
        renderer: Renderer,
        scene: LoadedSnapshot,
        player: Option<BrowserPlayer>,
    }
    struct BrowserPlayer {
        runtime: rrising_core::diagnostic_player::DiagnosticPlayer,
        visual_items: Vec<usize>,
        initial: [f32; 3],
        base_uniforms: FrameUniforms,
        yaw: f32,
        animation: Option<BrowserAnimation>,
    }
    struct BrowserAnimation {
        poses: SnapshotPlayerAnimation,
        clock: rrising_core::diagnostic_player::MotionAnimation,
        frame_count: usize,
        last_frame: Option<usize>,
    }
    fn js_error(e: impl std::fmt::Display) -> JsValue {
        JsValue::from_str(&e.to_string())
    }
    #[wasm_bindgen]
    impl BrowserViewer {
        pub async fn create(
            canvas: web_sys::HtmlCanvasElement,
            json: String,
        ) -> Result<BrowserViewer, JsValue> {
            if json.len() > 256 * 1024 * 1024 {
                return Err(js_error("scene transfer exceeds 256 MiB"));
            }
            let mut snapshot: Snapshot = serde_json::from_str(&json).map_err(js_error)?;
            let player_data = snapshot.diagnostic_player.take();
            let renderer = Renderer::from_canvas(canvas).await.map_err(js_error)?;
            let mut scene = snapshot.upload(&renderer).map_err(js_error)?;
            let player = if let Some(data) = player_data {
                if !data.camera_yaw.is_finite() {
                    return Err(js_error("nonfinite camera yaw"));
                }
                let components = rrising_core::diagnostic_player::PlayerComponents {
                    input: data.input,
                    movement: data.movement,
                    velocity: data.velocity,
                    move_velocity: data.move_velocity,
                    translation: data.translation,
                    spell_controlled: data.spell_controlled,
                };
                let runtime =
                    rrising_core::diagnostic_player::DiagnosticPlayer::from_components(&components)
                        .map_err(js_error)?;
                let initial = runtime.position().map_err(js_error)?;
                let base_uniforms = scene.uniforms;
                scene
                    .set_diagnostic_player_position(
                        &data.visual_items,
                        initial,
                        initial,
                        base_uniforms,
                    )
                    .map_err(js_error)?;
                let animation = if let Some(poses) = data.animation {
                    let frame_count = scene
                        .validate_player_animation(&data.visual_items, &poses)
                        .map_err(js_error)?;
                    scene
                        .set_player_animation(&renderer, &data.visual_items, &poses, None)
                        .map_err(js_error)?;
                    Some(BrowserAnimation {
                        poses,
                        clock: rrising_core::diagnostic_player::MotionAnimation::new(initial),
                        frame_count,
                        last_frame: None,
                    })
                } else {
                    None
                };
                Some(BrowserPlayer {
                    runtime,
                    visual_items: data.visual_items,
                    initial,
                    base_uniforms,
                    yaw: data.camera_yaw,
                    animation,
                })
            } else {
                None
            };
            Ok(Self {
                renderer,
                scene,
                player,
            })
        }
        pub fn render(&mut self) -> Result<(), JsValue> {
            self.scene.render(&mut self.renderer).map_err(js_error)
        }
        pub fn item_count(&self) -> usize {
            self.scene.item_count()
        }
        pub fn has_player(&self) -> bool {
            self.player.is_some()
        }
        pub fn player_position(&self) -> Result<Vec<f32>, JsValue> {
            self.player
                .as_ref()
                .map(|p| p.runtime.position().map(|v| v.to_vec()).map_err(js_error))
                .unwrap_or_else(|| Ok(Vec::new()))
        }
        /// -1 means rest pose or no exported animation.
        pub fn player_animation_frame(&self) -> i32 {
            self.player
                .as_ref()
                .and_then(|p| p.animation.as_ref())
                .and_then(|a| a.last_frame)
                .map_or(-1, |f| f as i32)
        }
        /// Approximate diagnostic movement only; the original scheduler and collision are absent.
        pub fn advance_player(&mut self, x: f32, z: f32, elapsed_ms: f64) -> Result<u32, JsValue> {
            if !elapsed_ms.is_finite() || elapsed_ms < 0.0 {
                return Err(js_error("invalid frame time"));
            }
            let Some(player) = &mut self.player else {
                return Ok(0);
            };
            let ns = (elapsed_ms.min(250.0) * 1_000_000.0) as u64;
            let steps = player
                .runtime
                .advance([x, z], player.yaw, ns)
                .map_err(js_error)?;
            if steps > 0 {
                let current = player.runtime.position().map_err(js_error)?;
                self.scene
                    .set_diagnostic_player_position(
                        &player.visual_items,
                        player.initial,
                        current,
                        player.base_uniforms,
                    )
                    .map_err(js_error)?;
                if let Some(animation) = &mut player.animation {
                    animation.clock.advance(
                        current,
                        steps as f32 * rrising_core::diagnostic_player::STEP_NS as f32 * 1e-9,
                        animation.poses.duration,
                    );
                    let frame = animation
                        .clock
                        .frame(animation.poses.duration, animation.frame_count);
                    if frame != animation.last_frame {
                        self.scene
                            .set_player_animation(
                                &self.renderer,
                                &player.visual_items,
                                &animation.poses,
                                frame,
                            )
                            .map_err(js_error)?;
                        animation.last_frame = frame;
                    }
                }
            }
            Ok(steps)
        }
    }
}
