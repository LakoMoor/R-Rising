//! Cross-checks the server's `[ExcludeComponent]` option lists against component names saved in
//! original autosaves. Needs RRISING_SAVE_DIR and RRISING_EXCLUDE_JSON (local, not distributed).
use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use rrising_content::persistence::{PersistentSave, SavedSnapshot};

#[test]
#[ignore = "requires RRISING_SAVE_DIR and RRISING_EXCLUDE_JSON"]
fn saved_components_versus_exclude_options() -> Result<()> {
    let folder =
        PathBuf::from(std::env::var_os("RRISING_SAVE_DIR").context("set RRISING_SAVE_DIR")?);
    let lists: HashMap<String, Vec<String>> = serde_json::from_slice(&std::fs::read(
        std::env::var_os("RRISING_EXCLUDE_JSON").context("set RRISING_EXCLUDE_JSON")?,
    )?)?;
    let option_of: HashMap<&str, &str> = lists
        .iter()
        .flat_map(|(k, names)| names.iter().map(move |n| (n.as_str(), k.as_str())))
        .collect();
    let name = std::env::var("RRISING_SAVE_FILE").unwrap_or("AutoSave_1.save.gz".into());
    let save = PersistentSave::open(&folder.join(name))?;
    let snap = SavedSnapshot::read(&save)?;
    let mut tally: HashMap<&str, usize> = HashMap::new();
    let mut matched = 0;
    for c in &snap.header().required_components {
        if let Some(opt) = option_of.get(c.full_name.as_str()) {
            matched += 1;
            *tally.entry(opt).or_default() += 1;
        }
    }
    println!(
        "required components {}, matched to exclude lists {matched}, per option {tally:?}",
        snap.header().required_components.len()
    );
    Ok(())
}
