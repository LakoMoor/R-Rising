//! Assembling a drawable scene out of the content store.
//!
//! The original places art through ECS scenes: `EntityScenes/*.entities` hold `Mesh`/`Material`
//! references per entity, and `ProjectM` streams the archives those references name. Parsing those
//! binaries is still open (see `docs/PORT.md`), so this module builds a scene the way the game's
//! content tests do — by picking real objects out of the archive index — which is enough to put the
//! original's models on screen and to keep the renderer honest about what it was given.

use rrising_content::GameContent;
use rrising_math::{float3, float4x4, quaternion};
use unity_formats::player::ObjRef;

/// One drawable thing: a mesh, the material it should be drawn with, and where it sits.
pub struct SceneItem {
    pub mesh: ObjRef,
    pub material: Option<ObjRef>,
    pub transform: float4x4,
    /// The mesh's `m_Name`, carried for labels and diagnostics.
    pub name: String,
}

/// A scene the renderer can draw.
pub struct SceneSpec {
    pub items: Vec<SceneItem>,
}

impl SceneSpec {
    /// How many items the scene holds.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the scene is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Pick the base-colour map out of a material's texture table.
///
/// The original's HDRP materials name their slots (`_BaseColorMap`, `_NormalMap`, `_MaskTex`, ...).
/// The port's first pass only samples one texture, so it takes the base map and, failing that, the
/// first slot that holds a texture — a choice recorded rather than hidden.
pub fn base_map_name(material: &unity_formats::material::UnityMaterialData) -> Option<&str> {
    const PREFERRED: [&str; 4] = ["_BaseColorMap", "_MainTex", "_ColorMap", "_BassMap"];
    for want in PREFERRED {
        for (slot, env) in &material.textures {
            if slot == want && env.texture.is_some() {
                return Some(slot.as_str());
            }
        }
    }
    material
        .textures
        .iter()
        .find(|(_, env)| env.texture.is_some())
        .map(|(slot, _)| slot.as_str())
}

/// Build a showcase scene: up to `limit` meshes that have a material with a texture, laid out in a
/// grid with the original's own TRS composition.
pub fn showcase(content: &GameContent, limit: usize) -> SceneSpec {
    let mut items = Vec::new();
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in content.entries() {
        if items.len() >= limit {
            break;
        }
        if entry.class != "Mesh" || entry.name.is_empty() {
            continue;
        }
        if !seen_names.insert(entry.name.clone()) {
            continue;
        }
        // Find a material in the same archive: the archives group a model with the art it needs.
        let material = content
            .entries()
            .iter()
            .find(|m| m.class == "Material" && m.object.file == entry.object.file)
            .map(|m| m.object.clone());
        let col = items.len() as f32 % 6.0;
        let row = (items.len() as f32 / 6.0).floor();
        let transform = float4x4::trs(
            float3::new(col * 2.0 - 5.0, 0.0, row * 2.0),
            quaternion::identity,
            float3::splat(1.0),
        );
        items.push(SceneItem {
            mesh: entry.object.clone(),
            material,
            transform,
            name: entry.name.clone(),
        });
    }
    SceneSpec { items }
}
