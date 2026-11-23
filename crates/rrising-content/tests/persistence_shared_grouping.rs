//! Measures how saved job archetypes carrying shared-component identities relate to the
//! original's chunk grouping. Needs RRISING_SAVE_DIR with original reference autosaves.
use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use rrising_content::persistence::{PersistentSave, SavedSnapshot};

#[test]
#[ignore = "requires RRISING_SAVE_DIR with original reference autosaves"]
fn shared_identity_archetypes_chunk_grouping_report() -> Result<()> {
    let folder =
        PathBuf::from(std::env::var_os("RRISING_SAVE_DIR").context("set RRISING_SAVE_DIR")?);
    let name = std::env::var("RRISING_SAVE_FILE").unwrap_or("AutoSave_4.save.gz".into());
    let save = PersistentSave::open(&folder.join(name))?;
    let snap = SavedSnapshot::read(&save)?;
    let world = snap.world_data();
    // world archetype chunk-size signatures, keyed by (hash count, sizes)
    let mut world_sigs: BTreeMap<Vec<usize>, usize> = BTreeMap::new();
    if let Some(archs) = &world.nonpersistent_archetypes {
        for a in archs {
            let sizes: Vec<usize> = a.chunks.iter().map(|c| c.entity_indices().len()).collect();
            *world_sigs.entry(sizes).or_default() += 1;
        }
    }
    let (mut with_shared, mut multi_chunk_matches, mut total) = (0, 0, 0);
    for (_, job) in snap.jobs() {
        for a in &job.archetypes {
            total += 1;
            if !a.types.iter().any(|t| t.is_shared_component_type()) {
                continue;
            }
            with_shared += 1;
            let sizes: Vec<usize> = a.chunks.iter().map(|c| c.entities().len()).collect();
            if world_sigs.contains_key(&sizes) {
                multi_chunk_matches += 1;
            }
        }
    }
    println!("job archetypes {total}, with shared identity {with_shared}, chunk-size signature present in world data {multi_chunk_matches}");
    Ok(())
}
