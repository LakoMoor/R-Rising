//! Find candidate HUD resources by name; name matches alone do not prove use in the game.
use anyhow::Result;
use rrising_content::{list_archives, GameContent};
use std::path::Path;

fn main() -> Result<()> {
    let data = std::env::var("VRISING_DATA")?;
    let archives = list_archives(&Path::new(&data).join("StreamingAssets/ContentArchives"))?;
    let content = GameContent::open(
        Path::new(&data),
        &archives,
        Path::new("assets/typetrees.tpk"),
    )?;
    for entry in content.entries().iter().filter(|entry| {
        let name = entry.name.to_ascii_lowercase();
        matches!(entry.class.as_str(), "GameObject" | "Sprite" | "Texture2D")
            && [
                "hud",
                "actionbar",
                "action_bar",
                "hotbar",
                "hot_bar",
                "healthbar",
                "health_bar",
                "minimap",
            ]
            .iter()
            .any(|term| name.contains(term))
    }) {
        println!(
            "{}\t{}\t{}\t{}",
            entry.class, entry.name, entry.object.file, entry.object.path_id
        );
    }
    Ok(())
}
