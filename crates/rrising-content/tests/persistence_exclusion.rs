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
    let names: HashMap<_, _> = snap
        .header()
        .required_components
        .iter()
        .map(|c| (c.saved_type_index, c.full_name.as_str()))
        .collect();
    let mut column_rows: HashMap<&str, usize> = HashMap::new();
    let mut other_rows = 0usize;
    for (_, job) in snap.jobs() {
        for a in &job.archetypes {
            for chunk in &a.chunks {
                for col in &chunk.columns {
                    let rows = chunk.entities().len();
                    match names
                        .get(&col.saved_type_index)
                        .and_then(|n| option_of.get(n))
                    {
                        Some(opt) => *column_rows.entry(opt).or_default() += rows,
                        None => other_rows += rows,
                    }
                }
            }
        }
    }
    println!("saved column rows by option {column_rows:?}, unlisted {other_rows}");
    if let Some(path) = std::env::var_os("RRISING_EXCLUDE_HASHES_JSON") {
        let by_hash: HashMap<String, Vec<String>> = serde_json::from_slice(&std::fs::read(path)?)?;
        let hash_option: HashMap<u64, &str> = by_hash
            .iter()
            .flat_map(|(k, hs)| {
                hs.iter()
                    .filter_map(move |h| u64::from_str_radix(h, 16).ok().map(|h| (h, k.as_str())))
            })
            .collect();
        let mut rows: HashMap<&str, usize> = HashMap::new();
        let mut types: HashMap<&str, std::collections::BTreeSet<u64>> = HashMap::new();
        for (_, job) in snap.jobs() {
            for a in &job.archetypes {
                for chunk in &a.chunks {
                    for col in &chunk.columns {
                        if let Some(opt) = hash_option.get(&col.stable_type_hash) {
                            *rows.entry(opt).or_default() += chunk.entities().len();
                            types.entry(opt).or_default().insert(col.stable_type_hash);
                        }
                    }
                }
            }
        }
        let counts: HashMap<_, _> = types.iter().map(|(k, v)| (*k, v.len())).collect();
        println!("by stable hash: saved column rows {rows:?}, distinct saved types {counts:?}");
    }
    Ok(())
}
