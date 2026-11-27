//! A local content transfer, not a save game or an original scene format.
use crate::{
    mesh, texture::TextureData, DrawCall, FrameUniforms, GpuMesh, GpuTexture, Instance, Renderer,
    Vertex,
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};

/// Optional diagnostic frame art in a local transfer; this is not game HUD state.
#[derive(Serialize, Deserialize)]
pub struct SnapshotOverlay {
    pub texture: TextureData,
    pub layout: crate::overlay::ActionBarLayout,
}

/// Local-only component projection for the experimental player driver.
/// This is not a saved game or a complete ECS world.
#[derive(Serialize, Deserialize)]
pub struct SnapshotPlayer {
    pub input: Vec<u8>,
    pub movement: Vec<u8>,
    pub velocity: Vec<u8>,
    pub move_velocity: Vec<u8>,
    pub translation: Vec<u8>,
    pub spell_controlled: Option<Vec<u8>>,
    pub visual_items: Vec<usize>,
    pub camera_yaw: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub animation: Option<SnapshotPlayerAnimation>,
}

#[derive(Serialize, Deserialize)]
pub struct SnapshotPlayerAnimation {
    pub duration: f32,
    pub parts: Vec<SnapshotAnimationPart>,
}

#[derive(Serialize, Deserialize)]
pub struct SnapshotAnimationPart {
    pub rest: Vec<Vertex>,
    pub frames: Vec<Vec<Vertex>>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic_player: Option<SnapshotPlayer>,
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
    pub fn validate_player_animation(
        &self,
        indices: &[usize],
        animation: &SnapshotPlayerAnimation,
    ) -> Result<usize> {
        ensure!(
            animation.duration.is_finite() && animation.duration > 0.0,
            "invalid player animation duration"
        );
        ensure!(
            !indices.is_empty() && indices.len() <= 16 && animation.parts.len() == indices.len(),
            "invalid animation part count"
        );
        let count = animation.parts[0].frames.len();
        ensure!((1..=64).contains(&count), "invalid animation sample count");
        let mut unique = std::collections::BTreeSet::new();
        for (&index, part) in indices.iter().zip(&animation.parts) {
            ensure!(unique.insert(index), "duplicate player visual index");
            let mesh = self
                .meshes
                .get(index)
                .context("player visual index out of range")?;
            ensure!(
                part.frames.len() == count
                    && part.rest.len() == mesh.vertex_count
                    && part.frames.iter().all(|v| v.len() == mesh.vertex_count),
                "player animation topology mismatch"
            );
            for vertex in part.rest.iter().chain(part.frames.iter().flatten()) {
                ensure!(
                    vertex
                        .position
                        .iter()
                        .chain(&vertex.normal)
                        .chain(&vertex.uv)
                        .all(|v| v.is_finite()),
                    "nonfinite player animation vertex"
                );
            }
        }
        Ok(count)
    }

    /// Apply a validated diagnostic pose (`None` means serialized rest pose).
    pub fn set_player_animation(
        &self,
        renderer: &Renderer,
        indices: &[usize],
        animation: &SnapshotPlayerAnimation,
        frame: Option<usize>,
    ) -> Result<()> {
        ensure!(
            indices.len() == animation.parts.len() && !indices.is_empty(),
            "invalid animation part count"
        );
        for (&index, part) in indices.iter().zip(&animation.parts) {
            let mesh = self
                .meshes
                .get(index)
                .context("player visual index out of range")?;
            let vertices = match frame {
                Some(f) => part.frames.get(f).context("animation frame out of range")?,
                None => &part.rest,
            };
            ensure!(
                vertices.len() == mesh.vertex_count
                    && vertices.iter().all(|v| v
                        .position
                        .iter()
                        .chain(&v.normal)
                        .chain(&v.uv)
                        .all(|x| x.is_finite())),
                "invalid player pose"
            );
        }
        for (&index, part) in indices.iter().zip(&animation.parts) {
            let vertices = frame.map_or(part.rest.as_slice(), |f| part.frames[f].as_slice());
            mesh::update_vertices(renderer.queue(), &self.meshes[index], vertices)?;
        }
        Ok(())
    }
    /// Move the explicitly indexed player groups and a following diagnostic camera.
    /// `base_uniforms` and `initial` must be the original transfer values.
    pub fn set_diagnostic_player_position(
        &mut self,
        indices: &[usize],
        initial: [f32; 3],
        current: [f32; 3],
        base_uniforms: FrameUniforms,
    ) -> Result<()> {
        ensure!(
            initial.iter().chain(&current).all(|v| v.is_finite()),
            "nonfinite player position"
        );
        ensure!(
            !indices.is_empty() && indices.len() <= 16,
            "invalid player visual count"
        );
        let mut unique = std::collections::BTreeSet::new();
        for &index in indices {
            ensure!(unique.insert(index), "duplicate player visual index");
            let instance = self
                .instances
                .get(index)
                .context("player visual index out of range")?;
            ensure!(instance.len() == 1, "player visual must have one instance");
        }
        for &index in indices {
            self.instances[index][0].model[3][..3].copy_from_slice(&current);
        }
        let delta = std::array::from_fn::<_, 3, _>(|i| current[i] - initial[i]);
        self.uniforms = base_uniforms;
        for row in 0..4 {
            self.uniforms.view_proj[3][row] -= base_uniforms.view_proj[0][row] * delta[0]
                + base_uniforms.view_proj[1][row] * delta[1]
                + base_uniforms.view_proj[2][row] * delta[2];
        }
        for (axis, value) in delta.iter().enumerate() {
            self.uniforms.camera_pos[axis] += value;
        }
        Ok(())
    }
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
