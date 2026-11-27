//! A local content transfer, not a save game or an original scene format.
use crate::{
    mesh, texture::TextureData, DrawCall, FrameUniforms, GpuMesh, GpuTexture, Instance, Renderer,
    Vertex,
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

/// Optional diagnostic frame art in a local transfer; this is not game HUD state.
#[derive(Serialize, Deserialize)]
pub struct SnapshotOverlay {
    pub texture: TextureData,
    pub layout: crate::overlay::ActionBarLayout,
}

#[derive(Serialize, Deserialize)]
pub struct SnapshotItem {
    pub name: String,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub transform: Instance,
    /// Extra placements of the same mesh and texture (instanced scenes). Empty for a single
    /// object; when non-empty these replace `transform`.
    #[serde(default)]
    pub instances: Vec<Instance>,
    pub texture: Option<TextureData>,
    /// Index into [`Snapshot::textures`], for scenes where many items share one texture.
    /// Ignored when `texture` is present.
    #[serde(default)]
    pub texture_index: Option<usize>,
    /// Alpha-tested material (`fs_cutout`). Absent in older transfers, which means opaque.
    #[serde(default)]
    pub cutout: bool,
}
#[derive(Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub description: String,
    pub uniforms: FrameUniforms,
    pub items: Vec<SnapshotItem>,
    /// Shared texture pool referenced by `SnapshotItem::texture_index`.
    #[serde(default)]
    pub textures: Vec<TextureData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic_overlay: Option<SnapshotOverlay>,
}
pub struct LoadedSnapshot {
    meshes: Vec<GpuMesh>,
    textures: Vec<Option<GpuTexture>>,
    pool: Vec<GpuTexture>,
    item_pool: Vec<Option<usize>>,
    cutout: Vec<bool>,
    instances: Vec<Vec<Instance>>,
    pub uniforms: FrameUniforms,
    overlay: Option<(GpuTexture, crate::overlay::ActionBarLayout)>,
}
impl Snapshot {
    pub fn upload(self, renderer: &Renderer) -> Result<LoadedSnapshot> {
        ensure!(
            self.version == 1,
            "unsupported scene transfer version {}",
            self.version
        );
        ensure!(
            !self.items.is_empty() && self.items.len() <= 4096,
            "invalid scene item count"
        );
        let mut loaded = LoadedSnapshot {
            meshes: vec![],
            textures: vec![],
            pool: vec![],
            item_pool: vec![],
            cutout: vec![],
            instances: vec![],
            uniforms: self.uniforms,
            overlay: None,
        };
        if let Some(overlay) = self.diagnostic_overlay {
            overlay.layout.validate()?;
            let texture = overlay.texture.upload(
                renderer.device(),
                renderer.queue(),
                renderer.device().features(),
            )?;
            loaded.overlay = Some((texture, overlay.layout));
        }
        for t in self.textures {
            loaded.pool.push(t.upload(
                renderer.device(),
                renderer.queue(),
                renderer.device().features(),
            )?);
        }
        for item in self.items {
            ensure!(
                item.texture_index.is_none_or(|i| i < loaded.pool.len()),
                "scene item {:?} references a missing pooled texture",
                item.name
            );
            loaded.item_pool.push(item.texture_index);
            loaded.cutout.push(item.cutout);
            loaded.meshes.push(mesh::upload_vertices(
                renderer.device(),
                &item.name,
                &item.vertices,
                &item.indices,
                1,
                false,
            )?);
            loaded.textures.push(
                item.texture
                    .map(|t| {
                        t.upload(
                            renderer.device(),
                            renderer.queue(),
                            renderer.device().features(),
                        )
                    })
                    .transpose()?,
            );
            loaded.instances.push(if item.instances.is_empty() {
                vec![item.transform]
            } else {
                item.instances
            });
        }
        Ok(loaded)
    }
}
impl LoadedSnapshot {
    pub fn render(&self, renderer: &mut Renderer) -> Result<()> {
        let neutral = renderer.neutral_view();
        let draws: Vec<_> = self
            .meshes
            .iter()
            .enumerate()
            .map(|(i, mesh)| DrawCall {
                mesh,
                instances: &self.instances[i],
                texture: self.textures[i]
                    .as_ref()
                    .or_else(|| self.item_pool[i].map(|p| &self.pool[p]))
                    .map(|t| &t.view)
                    .unwrap_or(&neutral),
                cutout: self.cutout[i],
            })
            .collect();
        let (w, h) = renderer.size();
        let rects = self
            .overlay
            .as_ref()
            .map_or_else(Vec::new, |(_, layout)| layout.rects(w, h));
        let overlays: Vec<_> = self
            .overlay
            .as_ref()
            .map(|(tex, _)| crate::overlay::OverlayDraw {
                texture: &tex.view,
                rects: &rects,
                uv: [0.0, 0.0, 1.0, 1.0],
            })
            .into_iter()
            .collect();
        renderer.render_surface_with_overlay(&self.uniforms, &draws, &overlays)
    }
    pub fn render_to_image(&self, renderer: &mut Renderer) -> Result<(u32, u32, Vec<u8>)> {
        let neutral = renderer.neutral_view();
        let draws: Vec<_> = self
            .meshes
            .iter()
            .enumerate()
            .map(|(i, mesh)| DrawCall {
                mesh,
                instances: &self.instances[i],
                texture: self.textures[i]
                    .as_ref()
                    .or_else(|| self.item_pool[i].map(|p| &self.pool[p]))
                    .map(|t| &t.view)
                    .unwrap_or(&neutral),
                cutout: self.cutout[i],
            })
            .collect();
        let (w, h) = renderer.size();
        let rects = self
            .overlay
            .as_ref()
            .map_or_else(Vec::new, |(_, layout)| layout.rects(w, h));
        let overlays: Vec<_> = self
            .overlay
            .as_ref()
            .map(|(tex, _)| crate::overlay::OverlayDraw {
                texture: &tex.view,
                rects: &rects,
                uv: [0.0, 0.0, 1.0, 1.0],
            })
            .into_iter()
            .collect();
        renderer.render_to_image_with_overlay(&self.uniforms, &draws, &overlays)
    }
    pub fn item_count(&self) -> usize {
        self.meshes.len()
    }
}
