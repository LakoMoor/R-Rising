//! `StreamingAssets/Localization/*.json` — the game's string tables.
//!
//! Each language file is a JSON array of keyed strings. The HUD and every UI screen resolve text
//! through these keys, so the port reads them rather than baking English into the UI.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};

/// One language's strings, keyed by the id the UI asks for.
#[derive(Clone, Debug, Default)]
pub struct LocalizationTable {
    /// Language file name without its extension (`English`).
    pub language: String,
    /// Key -> text, kept in file order as well as keyed for lookup.
    pub strings: BTreeMap<String, String>,
}

impl LocalizationTable {
    /// Read one language file.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        let value: serde_json::Value = serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", path.display()))?;
        let mut table = LocalizationTable {
            language: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            strings: BTreeMap::new(),
        };
        collect(&value, &String::new(), &mut table.strings);
        Ok(table)
    }

    /// The text for `key`, or `None` when the language has no such key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.strings.get(key).map(String::as_str)
    }

    /// Number of strings in the table.
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Whether the table holds no strings.
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }
}

/// Walk a localization document and collect every string leaf under its dotted path.
fn collect(value: &serde_json::Value, path: &str, out: &mut BTreeMap<String, String>) {
    match value {
        serde_json::Value::String(s) => {
            if !path.is_empty() {
                out.insert(path.to_string(), s.clone());
            }
        }
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let child = if path.is_empty() {
                    k.clone()
                } else {
                    format!("{path}.{k}")
                };
                collect(v, &child, out);
            }
        }
        serde_json::Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                let child = format!("{path}[{i}]");
                collect(v, &child, out);
            }
        }
        _ => {}
    }
}
