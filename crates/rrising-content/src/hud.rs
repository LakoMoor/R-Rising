//! Bounded extraction of the pinned client's action-bar frame asset.
//! This does not implement Unity Canvas, item state, interaction or HUD behavior.
use crate::GameContent;
use anyhow::{ensure, Context, Result};
use unity_formats::{
    player::{ObjRef, PPtr},
    scene::Scene,
    value::Value,
};

pub struct ActionBarFrame {
    pub texture: ObjRef,
    pub slot_size: [f32; 2],
    pub frame_extra: [f32; 2],
    pub anchor_position: [f32; 2],
    pub spacing: f32,
    pub count: usize,
}

fn vec2(value: &Value, field: &str) -> Result<[f32; 2]> {
    let get = |axis| {
        value
            .at(&[field, axis])
            .and_then(Value::as_f32)
            .with_context(|| format!("missing {field}.{axis}"))
    };
    let result = [get("x")?, get("y")?];
    ensure!(result.iter().all(|v| v.is_finite()), "nonfinite {field}");
    Ok(result)
}

/// Follow the original ActionBar prefab, its Frame Images, and the Sprite texture.
/// The Image payload offset and supported sprite shape are specific to this pinned build.
pub fn candidate_action_bar(content: &GameContent) -> Result<ActionBarFrame> {
    const FILE: &str = "26fae5e0b4c6a9c2db14710268c2d097";
    const ROOT: i64 = 5936793561428200050;
    let player = content.player();
    let scene = Scene::read(player, FILE)?;
    let root = scene.transform_of(ROOT).context("ActionBar root missing")?;
    let mut entries = Vec::new();
    let mut frames = Vec::new();
    for &id in scene.game_objects.keys() {
        let Some(transform) = scene.transform_of(id) else {
            continue;
        };
        let Some(path) = scene.path_below(transform, root) else {
            continue;
        };
        if !path.contains('/') && path.starts_with("ActionBarEntry") {
            entries.push(id);
        }
        if path.starts_with("ActionBarEntry") && path.ends_with("/Background/Frame") {
            frames.push(id);
        }
    }
    ensure!(entries.len() == 8, "unexpected action bar entry count");
    ensure!(
        frames.len() == entries.len(),
        "unexpected action bar frame count"
    );
    let first = entries.iter().copied().find(|id| {
        scene
            .game_objects
            .get(id)
            .is_some_and(|o| o.name == "ActionBarEntry")
    });
    let first = first.context("first action bar entry missing")?;
    let rect = |id| -> Result<Value> {
        let path_id = scene.transform_of(id).context("missing RectTransform")?;
        let r = ObjRef {
            file: FILE.into(),
            path_id,
        };
        ensure!(
            player.class_name(&r) == Some("RectTransform"),
            "expected RectTransform"
        );
        Ok(player.decode(&r)?)
    };
    let slot_size = vec2(&rect(first)?, "m_SizeDelta")?;
    let frame_extra = vec2(&rect(frames[0])?, "m_SizeDelta")?;
    for &id in &entries {
        ensure!(
            vec2(&rect(id)?, "m_SizeDelta")? == slot_size,
            "slot sizes differ"
        );
    }
    let anchor_position = vec2(&rect(ROOT)?, "m_AnchoredPosition")?;
    let root_object = scene.game_objects.get(&ROOT).context("root object")?;
    let mut spacing = None;
    for &id in &root_object.components {
        let r = ObjRef {
            file: FILE.into(),
            path_id: id,
        };
        if player.class_name(&r) == Some("MonoBehaviour")
            && player.script_class(&r)?.as_deref() == Some("UnityEngine.UI.HorizontalLayoutGroup")
        {
            let (_, bytes) = player.behaviour(&r)?;
            ensure!(bytes.len() == 52, "unsupported horizontal layout payload");
            spacing = Some(f32::from_le_bytes(bytes[20..24].try_into()?));
        }
    }
    let spacing = spacing.context("missing horizontal layout")?;
    ensure!(spacing.is_finite() && spacing >= 0.0, "invalid spacing");
    let mut sprite = None;
    for frame_id in frames {
        ensure!(
            vec2(&rect(frame_id)?, "m_SizeDelta")? == frame_extra,
            "frame sizes differ"
        );
        let object = scene.game_objects.get(&frame_id).context("frame object")?;
        let mut image = None;
        for &id in &object.components {
            let r = ObjRef {
                file: FILE.into(),
                path_id: id,
            };
            if player.class_name(&r) == Some("MonoBehaviour")
                && player.script_class(&r)?.as_deref() == Some("UnityEngine.UI.Image")
            {
                let (_, bytes) = player.behaviour(&r)?;
                ensure!(bytes.len() == 104, "unsupported Image payload");
                let ptr = PPtr {
                    file_id: i32::from_le_bytes(bytes[56..60].try_into()?),
                    path_id: i64::from_le_bytes(bytes[60..68].try_into()?),
                };
                ensure!(
                    image
                        .replace(
                            player
                                .resolve_pptr(FILE, ptr)?
                                .context("null frame sprite")?
                        )
                        .is_none(),
                    "multiple frame images"
                );
            }
        }
        let image = image.context("missing frame image")?;
        if let Some(previous) = &sprite {
            ensure!(*previous == image, "frame sprites differ");
        } else {
            sprite = Some(image);
        }
    }
    let sprite = sprite.context("missing frame sprite")?;
    ensure!(
        player.class_name(&sprite) == Some("Sprite"),
        "frame is not a Sprite"
    );
    let value = player.decode(&sprite)?;
    ensure!(
        value.get("m_Name").and_then(Value::as_str) == Some("ItemFrame_Normal"),
        "unexpected frame sprite"
    );
    ensure!(
        value.at(&["m_RD", "settingsRaw"]).and_then(Value::as_i64) == Some(64),
        "unsupported packed sprite"
    );
    let texture_ptr = value
        .at(&["m_RD", "texture"])
        .context("missing sprite texture")?;
    let texture = player
        .resolve(&sprite.file, texture_ptr)?
        .context("null sprite texture")?;
    let tex = content.read_texture(&texture)?;
    let rect = value
        .at(&["m_RD", "textureRect"])
        .context("missing sprite rect")?;
    let number = |name| {
        rect.get(name)
            .and_then(Value::as_f32)
            .with_context(|| format!("missing sprite {name}"))
    };
    ensure!(
        number("x")? == 0.0
            && number("y")? == 0.0
            && number("width")? == tex.width as f32
            && number("height")? == tex.height as f32,
        "sprite subrect unsupported"
    );
    Ok(ActionBarFrame {
        texture,
        slot_size,
        frame_extra,
        anchor_position,
        spacing,
        count: entries.len(),
    })
}
