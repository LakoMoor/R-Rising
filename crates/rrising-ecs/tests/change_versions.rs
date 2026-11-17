//! Original native outputs captured before the Rust fix by tools/capture_change_versions.py.
use rrising_ecs::change_version::{did_change, increment_global_system_version};
use rrising_ecs::FrameContext;

fn cases() -> impl Iterator<Item = (u32, u32, bool, u32)> {
    include_str!("../../../evidence/oracles/change_versions.tsv")
        .lines()
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .map(|line| {
            let cols: Vec<_> = line.split('\t').collect();
            assert_eq!(cols.len(), 4);
            let hex = |i| u32::from_str_radix(cols[i], 16).unwrap();
            (hex(0), hex(1), cols[2] == "1", hex(3))
        })
}

#[test]
fn schedule_change_filter_matches_original_including_wrap_and_zero() {
    let mut count = 0;
    for (chunk, last, changed, _) in cases() {
        assert_eq!(did_change(chunk, last), changed);
        let ctx = FrameContext {
            frame: 0,
            delta_ns: 1,
            elapsed_ns: 0,
            system_version: 1,
            last_system_version: last,
        };
        assert_eq!(
            ctx.did_change(chunk),
            changed,
            "DidChange({chunk:#010x}, {last:#010x})"
        );
        count += 1;
    }
    assert_eq!(count, 2048);
}

#[test]
fn increment_matches_original_including_skipping_zero_after_wrap() {
    let mut count = 0;
    for (mut version, _, _, expected) in cases() {
        increment_global_system_version(&mut version);
        assert_eq!(version, expected);
        count += 1;
    }
    assert_eq!(count, 2048);
}
