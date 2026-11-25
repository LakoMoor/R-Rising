//! Replays the original AimPrediction oracle (`evidence/oracles/aim_prediction.tsv`).
use rrising_core::aim::{first_order_intercept, first_order_intercept_time};

fn f(hex: &str) -> f32 {
    f32::from_bits(u32::from_str_radix(hex, 16).expect("hex"))
}

fn same(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

#[test]
fn matches_original_replay() {
    let text = include_str!("../../../evidence/oracles/aim_prediction.tsv");
    let rows: Vec<Vec<&str>> = text
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.split('\t').collect())
        .collect();
    let (mut time_ok, mut point_ok, mut cases) = (0, 0, 0);
    for a in &rows {
        let speed = f(a[0]);
        let t = first_order_intercept_time(speed, [f(a[1]), f(a[2])], [f(a[3]), f(a[4])]);
        cases += 1;
        time_ok += usize::from(same(t, f(a[5])));
        let p = first_order_intercept(
            [f(a[6]), f(a[7])],
            [f(a[8]), f(a[9])],
            speed,
            [f(a[10]), f(a[11])],
            [f(a[12]), f(a[13])],
        );
        point_ok += usize::from(same(p[0], f(a[14])) && same(p[1], f(a[15])));
    }
    println!("cases {cases}: time exact {time_ok}, point exact {point_ok}");
    assert_eq!((time_ok, point_ok), (cases, cases));
}
