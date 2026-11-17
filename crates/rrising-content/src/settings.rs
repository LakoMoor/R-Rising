//! `StreamingAssets/GameSettingPresets`, `GameDifficultyPresets` and `Settings`.
//!
//! A world's rules come from these JSON files: the preset picked at world creation, the difficulty
//! multipliers, and the client/server settings. The port reads them as untyped JSON trees so the
//! gameplay layer can pull the values it needs by path, which is what the original's C# does through
//! its own serializer.

use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

/// Read one settings document.
pub fn load_json(path: &Path) -> Result<Value> {
    // The game writes these with a UTF-8 BOM, which serde_json rejects.
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    serde_json::from_str(text).with_context(|| format!("parsing {}", path.display()))
}

/// Follow a dotted path like `GameSettings.XpMultiplier` through a settings document.
pub fn lookup<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = root;
    for part in path.split('.') {
        cur = match cur {
            Value::Object(map) => map.get(part)?,
            Value::Array(items) => {
                let idx: usize = part.parse().ok()?;
                items.get(idx)?
            }
            _ => return None,
        };
    }
    Some(cur)
}

/// List the available preset names in a folder, without their `.json` suffix.
pub fn list_presets(dir: &Path) -> Result<Vec<String>> {
    let mut out: Vec<String> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension().and_then(|x| x.to_str()) == Some("json")).then(|| {
                p.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            })
        })
        .collect();
    out.sort();
    Ok(out)
}
