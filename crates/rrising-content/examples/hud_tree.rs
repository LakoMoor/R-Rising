//! Read-only inventory of selected original HUD hierarchy and component shapes.
use anyhow::{Context, Result};
use rrising_content::{list_archives, GameContent};
use std::path::Path;
use unity_formats::{player::ObjRef, scene::Scene};

fn main() -> Result<()> {
    let data = std::env::var("VRISING_DATA")?;
    let archives = list_archives(&Path::new(&data).join("StreamingAssets/ContentArchives"))?;
    let content = GameContent::open(
        Path::new(&data),
        &archives,
        Path::new("assets/typetrees.tpk"),
    )?;
    let file = "26fae5e0b4c6a9c2db14710268c2d097";
    let scene = Scene::read(content.player(), file)?;
    let root = scene
        .transform_of(5936793561428200050)
        .context("ActionBar")?;
    for (id, object) in &scene.game_objects {
        let Some(transform) = scene.transform_of(*id) else {
            continue;
        };
        let Some(path) = scene.path_below(transform, root) else {
            continue;
        };
        println!("node {path} id={id} active={}", object.active);
        for &component in &object.components {
            let r = ObjRef {
                file: file.into(),
                path_id: component,
            };
            let class = content.player().class_name(&r).unwrap_or("unknown");
            if class == "RectTransform" {
                let value = content.player().decode(&r)?;
                println!("  rect {:?}", value);
            } else if class == "MonoBehaviour" {
                let script = content.player().script_class(&r)?;
                let (_, bytes) = content.player().behaviour(&r)?;
                println!("  script {script:?} bytes={}", bytes.len());
                if path.is_empty()
                    && script.as_deref() == Some("UnityEngine.UI.HorizontalLayoutGroup")
                {
                    for (i, b) in bytes.as_chunks::<4>().0.iter().enumerate() {
                        println!(
                            "  layout offset={} int={} float={}",
                            i * 4,
                            i32::from_le_bytes(*b),
                            f32::from_le_bytes(*b)
                        );
                    }
                }
                if script.as_deref() == Some("UnityEngine.UI.Image") {
                    for at in (0..bytes.len().saturating_sub(11)).step_by(4) {
                        let file_id = i32::from_le_bytes(bytes[at..at + 4].try_into()?);
                        let path_id = i64::from_le_bytes(bytes[at + 4..at + 12].try_into()?);
                        if path_id == 0 {
                            continue;
                        }
                        if let Ok(Some(target)) = content
                            .player()
                            .resolve_pptr(file, unity_formats::player::PPtr { file_id, path_id })
                        {
                            if content.player().class_name(&target) == Some("Sprite") {
                                let sprite = content.player().decode(&target)?;
                                println!(
                                    "  candidate sprite offset={at} target={target:?} name={:?}",
                                    sprite.get("m_Name").and_then(|v| v.as_str())
                                );
                                if path == "ActionBarEntry/Background/Frame" {
                                    println!(
                                        "  sprite rect={:?} rd_rect={:?} tex={:?} settings={:?}",
                                        sprite.get("m_Rect"),
                                        sprite.at(&["m_RD", "textureRect"]),
                                        sprite.at(&["m_RD", "texture"]),
                                        sprite.at(&["m_RD", "settingsRaw"])
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
