//! A fingerprint of the whole content index, so a reader change cannot silently move objects.
//!
//! The content readers either find the same objects with the same names in the same files, or they
//! do not. This test builds the full index of the install, sorts it, and hashes it with FNV-1a-64;
//! the recorded value and the exact recipe are in `evidence/index_fingerprint.txt`. It is a
//! regression guard, not a claim about the original: the fingerprint says nothing about whether the
//! index is *right*, only about whether it changed.

use std::path::PathBuf;

use rrising_content::{list_archives, GameContent};

/// FNV-1a, 64-bit: enough to notice a change, and reproducible without a hashing dependency.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn install() -> PathBuf {
    PathBuf::from("/Users/lakomoor/ported/VRising/VRising_Data")
}

#[test]
fn content_index_fingerprint_is_stable() {
    let data = install();
    let dir = data.join("StreamingAssets/ContentArchives");
    let Ok(all) = list_archives(&dir) else {
        println!("no install at {}, skipping", dir.display());
        return;
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tpk = root.join("assets/typetrees.tpk");
    let content =
        GameContent::open(&data, &all, &tpk).expect("open the install's content archives");

    // One line per indexed object: location, class id, class name, stored name. Sorted so the hash
    // does not depend on the order archives were opened in.
    let mut lines: Vec<String> = content
        .entries()
        .iter()
        .map(|e| format!("{:?}\t{}\t{}\t{}", e.object, e.class_id, e.class, e.name))
        .collect();
    lines.sort();
    let joined = lines.join("\n");
    let got = format!(
        "objects={}\nbytes={}\nfnv1a64={:016x}",
        lines.len(),
        joined.len(),
        fnv1a64(joined.as_bytes())
    );

    let record = root.join("evidence/index_fingerprint.txt");
    if !record.exists() {
        println!("{got}");
        panic!(
            "no recorded fingerprint at {}; the block above is what this run computed",
            record.display()
        );
    }
    // The record file carries its own recipe as `#` comments; only the value lines are compared.
    let want: String = std::fs::read_to_string(&record)
        .expect("read the recorded fingerprint")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        want.trim(),
        got.trim(),
        "the content index changed; re-measure and update {} if that was intended",
        record.display()
    );
}
