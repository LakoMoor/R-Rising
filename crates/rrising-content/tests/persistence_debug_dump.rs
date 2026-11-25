//! Compares the saved job archetype index with the original server's own persistence debug dump
//! (`AutoSave_N.save.gz_DebugDump.txt`): chunks, entities and included component names per
//! archetype (the saved type list must lie between Incl and Incl+Excl). Needs RRISING_SAVE_DIR and RRISING_SAVE_FILE (the dump sits next to the save).
use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use rrising_content::persistence::{PersistentSave, SavedSnapshot};

type DumpRow = (usize, usize, Vec<String>, Vec<String>);

fn parse_rows(text: &str) -> Vec<DumpRow> {
    let mut rows = Vec::new();
    let mut sections = 0;
    for line in text.lines() {
        if line.starts_with('[') {
            sections += 1;
        }
        if sections > 2 {
            break;
        }
        let Some((head, rest)) = line.split_once(": Archetype Incl(") else {
            continue;
        };
        let Some((incl, excl)) = rest.split_once("), Excl(") else {
            continue;
        };
        let nums: Vec<usize> = head
            .split(',')
            .filter_map(|p| {
                p.split_whitespace()
                    .filter(|t| t.chars().all(|c| c.is_ascii_digit()))
                    .collect::<String>()
                    .parse()
                    .ok()
            })
            .collect();
        if nums.len() != 3 {
            continue;
        }
        let split = |s: &str| -> Vec<String> {
            let mut v: Vec<String> = s
                .trim_end()
                .trim_end_matches(')')
                .split(',')
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect();
            v.sort();
            v
        };
        rows.push((nums[1], nums[2], split(incl), split(excl)));
    }
    rows
}

#[test]
#[ignore = "requires RRISING_SAVE_DIR/RRISING_SAVE_FILE with the original debug dump beside the save"]
fn saved_type_lists_lie_between_dump_included_and_included_plus_excluded() -> Result<()> {
    let dir = PathBuf::from(std::env::var_os("RRISING_SAVE_DIR").context("RRISING_SAVE_DIR")?);
    let file = std::env::var("RRISING_SAVE_FILE").context("RRISING_SAVE_FILE")?;
    let mut dump = parse_rows(&std::fs::read_to_string(
        dir.join(format!("{file}_DebugDump.txt")),
    )?);
    let save = PersistentSave::open(&dir.join(&file))?;
    let snap = SavedSnapshot::read(&save)?;
    let names: BTreeMap<_, _> = snap
        .header()
        .required_components
        .iter()
        .map(|c| {
            (
                c.saved_type_index,
                c.full_name
                    .rsplit(['.', '+'])
                    .next()
                    .unwrap_or("")
                    .to_owned(),
            )
        })
        .collect();
    let (mut ok, mut total) = (0, 0);
    for (_, job) in snap.jobs() {
        for a in &job.archetypes {
            total += 1;
            let entities: usize = a.chunks.iter().map(|c| c.entities().len()).sum();
            let types: Vec<&String> = a.types.iter().filter_map(|t| names.get(t)).collect();
            let hit = dump.iter().position(|(c, e, incl, excl)| {
                *c == a.chunks.len()
                    && *e == entities
                    && incl.iter().all(|n| types.contains(&n))
                    && types.iter().all(|n| incl.contains(n) || excl.contains(n))
            });
            if let Some(i) = hit {
                dump.swap_remove(i);
                ok += 1;
            }
        }
    }
    println!("archetypes {total}, one-to-one matched with Incl <= saved types <= Incl+Excl: {ok}");
    Ok(())
}

#[test]
#[ignore = "requires RRISING_SAVE_DIR/RRISING_SAVE_FILE with the original debug dump beside the save"]
fn archetype_byte_spans_versus_dump_sizes() -> Result<()> {
    let dir = PathBuf::from(std::env::var_os("RRISING_SAVE_DIR").context("RRISING_SAVE_DIR")?);
    let file = std::env::var("RRISING_SAVE_FILE").context("RRISING_SAVE_FILE")?;
    let text = std::fs::read_to_string(dir.join(format!("{file}_DebugDump.txt")))?;
    let mut sizes: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
    let mut sections = 0;
    for line in text.lines() {
        if line.starts_with('[') {
            sections += 1;
        }
        if sections > 2 {
            break;
        }
        if let Some((head, _)) = line.split_once(": Archetype Incl(") {
            let nums: Vec<usize> = head
                .split(',')
                .map(|p| {
                    p.split_whitespace()
                        .filter(|t| t.chars().all(|c| c.is_ascii_digit()))
                        .collect::<String>()
                        .parse()
                        .unwrap_or(0)
                })
                .collect();
            sizes.entry((nums[1], nums[2])).or_default().push(nums[0]);
        }
    }
    let save = PersistentSave::open(&dir.join(&file))?;
    let snap = SavedSnapshot::read(&save)?;
    let (mut exact, mut within_one, mut total) = (0, 0, 0);
    for (_, job) in snap.jobs() {
        for pair in job.archetypes.windows(2) {
            let a = &pair[0];
            let span_bits = pair[1].offset_bits - a.offset_bits;
            let entities: usize = a.chunks.iter().map(|c| c.entities().len()).sum();
            total += 1;
            let candidates = sizes.get(&(a.chunks.len(), entities));
            let bytes = (span_bits / 8) as usize;
            if candidates.is_some_and(|c| c.contains(&bytes)) {
                exact += 1;
            } else if candidates.is_some_and(|c| c.iter().any(|&s| s.abs_diff(bytes) <= 1)) {
                within_one += 1;
            }
        }
    }
    println!("archetypes with a following archetype {total}: byte span exact {exact}, within one byte {within_one}");
    Ok(())
}

#[test]
#[ignore = "requires RRISING_SAVE_DIR/RRISING_SAVE_FILE"]
fn snapshot_read_timing() -> Result<()> {
    let dir = PathBuf::from(std::env::var_os("RRISING_SAVE_DIR").context("RRISING_SAVE_DIR")?);
    let file = std::env::var("RRISING_SAVE_FILE").context("RRISING_SAVE_FILE")?;
    let mut ms = Vec::new();
    for _ in 0..5 {
        let start = std::time::Instant::now();
        let save = PersistentSave::open(&dir.join(&file))?;
        let snap = SavedSnapshot::read(&save)?;
        ms.push((start.elapsed().as_secs_f64() * 1000.0, snap.entity_count()));
    }
    ms.sort_by(|a, b| a.0.total_cmp(&b.0));
    println!(
        "open+read median {:.1} ms, entities {} (Rust only, release)",
        ms[2].0, ms[2].1
    );
    Ok(())
}
