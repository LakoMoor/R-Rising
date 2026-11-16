//! Browser host for the shared renderer. Not a separate game simulation.
#[cfg(target_arch = "wasm32")]
mod browser {
    use rrising_render::{
        snapshot::{LoadedSnapshot, Snapshot},
        Renderer,
    };
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub struct BrowserViewer {
        renderer: Renderer,
        scene: LoadedSnapshot,
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
            let snapshot: Snapshot = serde_json::from_str(&json).map_err(js_error)?;
            let renderer = Renderer::from_canvas(canvas).await.map_err(js_error)?;
            let scene = snapshot.upload(&renderer).map_err(js_error)?;
            Ok(Self { renderer, scene })
        }
        pub fn render(&mut self) -> Result<(), JsValue> {
            self.scene.render(&mut self.renderer).map_err(js_error)
        }
        pub fn item_count(&self) -> usize {
            self.scene.item_count()
        }
    }
}
