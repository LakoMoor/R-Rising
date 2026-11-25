//! Replays `evidence/oracles/easings.tsv` (original Motion2DEasings bytes) against the Rust port.
use rrising_core::easing::*;

type Easing = fn(f64, f64, f64, f64) -> f64;

fn f(hex: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(hex, 16).expect("hex"))
}

#[test]
fn matches_original_replay() {
    let fns: [(&str, Easing); 19] = [
        ("linear", linear),
        ("inQuad", in_quad),
        ("outQuad", out_quad),
        ("inOutQuad", in_out_quad),
        ("inCubic", in_cubic),
        ("outCubic", out_cubic),
        ("inOutCubic", in_out_cubic),
        ("inQuart", in_quart),
        ("outQuart", out_quart),
        ("inOutQuart", in_out_quart),
        ("inQuint", in_quint),
        ("outQuint", out_quint),
        ("inOutQuint", in_out_quint),
        ("inBack", in_back),
        ("outBack", out_back),
        ("inOutBack", in_out_back),
        ("inBounce", in_bounce),
        ("outBounce", out_bounce),
        ("inOutBounce", in_out_bounce),
    ];
    let text = include_str!("../../../evidence/oracles/easings.tsv");
    let mut bad = vec![0usize; fns.len()];
    let mut cases = 0;
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        let c: Vec<&str> = line.split('\t').collect();
        cases += 1;
        let (t, b, cc, d) = (f(c[0]), f(c[1]), f(c[2]), f(c[3]));
        for (i, (_, func)) in fns.iter().enumerate() {
            let (got, want) = (func(t, b, cc, d), f(c[4 + i]));
            if got.to_bits() != want.to_bits() && !(got.is_nan() && want.is_nan()) {
                bad[i] += 1;
            }
        }
    }
    let report: Vec<_> = fns
        .iter()
        .zip(&bad)
        .filter(|(_, n)| **n > 0)
        .map(|((n, _), k)| format!("{n}:{k}"))
        .collect();
    assert_eq!(cases, 2048);
    assert!(report.is_empty(), "mismatches {report:?}");
}
