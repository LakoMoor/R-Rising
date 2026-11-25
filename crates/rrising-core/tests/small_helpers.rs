//! Replays `evidence/oracles/small_helpers.tsv` from the original bytes.
use rrising_core::helpers::{blood_buff_progress, is_in_sun, sound_volumetric_lerp};

fn f(hex: &str) -> f32 {
    f32::from_bits(u32::from_str_radix(hex, 16).expect("hex"))
}

fn same(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

#[test]
fn matches_original_replay() {
    let text = include_str!("../../../evidence/oracles/small_helpers.tsv");
    let mut cases = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let c: Vec<&str> = line.split('\t').collect();
        cases += 1;
        assert_eq!(is_in_sun(f(c[0])), c[1] == "1", "sun {line}");
        assert!(
            same(blood_buff_progress(f(c[2]), f(c[3])), f(c[4])),
            "progress {line}"
        );
        let out = sound_volumetric_lerp(
            [f(c[5]), f(c[6]), f(c[7])],
            [f(c[8]), f(c[9]), f(c[10])],
            f(c[11]),
        );
        for k in 0..3 {
            assert!(same(out[k], f(c[12 + k])), "lerp {k} {line}");
        }
    }
    assert_eq!(cases, 4096);
}
