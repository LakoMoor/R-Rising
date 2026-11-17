//! Differential tests against Unity.Mathematics scalar leaves replayed from the original
//! GameAssembly.dll (x86-64 under Rosetta). Golden rows come from `tools/capture_math_int.py` and
//! `tools/capture_math_float.py`; only inputs and outputs are stored.
use rrising_math::*;

fn rows(src: &'static str) -> impl Iterator<Item = Vec<&'static str>> {
    src.lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| l.split('\t').collect())
}
fn hx(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}
fn dec(s: &str) -> i32 {
    s.parse().unwrap()
}
fn same_f32(got: f32, want: u32, what: &str, row: &[&str]) {
    let want = f32::from_bits(want);
    assert!(
        got.to_bits() == want.to_bits() || (got.is_nan() && want.is_nan()),
        "{what}: got {:08x} want {:08x} row {row:?}",
        got.to_bits(),
        want.to_bits()
    );
}

#[test]
fn integer_math_matches_original() {
    let mut n = 0;
    for r in rows(include_str!("../../../evidence/oracles/math_int.tsv")) {
        assert_eq!(r.len(), 14);
        let (x, a, b) = (hx(r[0]) as i32, hx(r[1]) as i32, hx(r[2]) as i32);
        assert_eq!(lzcnt(x), dec(r[3]), "lzcnt {x:x}");
        assert_eq!(reversebits(x) as u32, hx(r[4]), "reversebits {x:x}");
        assert_eq!(ceilpow2(x), dec_wrap(r[5]), "ceilpow2 {x:x}");
        assert_eq!(ceilpow2(x) as u32, hx(r[6]), "ceilpow2 (uint) {x:x}");
        assert_eq!(ceillog2(x), dec(r[7]), "ceillog2 {x:x}");
        assert_eq!(floorlog2(x), dec(r[8]), "floorlog2 {x:x}");
        assert_eq!(ispow2(x), r[9] == "1", "ispow2 {x:x}");
        assert_eq!(absi(x), dec(r[10]), "abs {x:x}");
        assert_eq!(mini(a, b), dec(r[11]), "min {a:x} {b:x}");
        assert_eq!(maxi(a, b), dec(r[12]), "max {a:x} {b:x}");
        assert_eq!(clampi(x, a, b), dec(r[13]), "clamp {x:x} {a:x} {b:x}");
        n += 1;
    }
    assert_eq!(n, 2048);
}
fn dec_wrap(s: &str) -> i32 {
    dec(s)
}

#[test]
fn float_math_matches_original() {
    let mut n = 0;
    for r in rows(include_str!("../../../evidence/oracles/math_float.tsv")) {
        assert_eq!(r.len(), 18);
        let f = |i: usize| f32::from_bits(hx(r[i]));
        let (a, b, c, d, e) = (f(0), f(1), f(2), f(3), f(4));
        let half = hx(r[5]) as i32;
        assert_eq!(f32tof16(a), hx(r[6]), "f32tof16 {:08x}", a.to_bits());
        same_f32(f16tof32(half), hx(r[7]), "f16tof32", &r);
        same_f32(radians(a), hx(r[8]), "radians", &r);
        same_f32(degrees(a), hx(r[9]), "degrees", &r);
        same_f32(unlerp(a, b, c), hx(r[10]), "unlerp", &r);
        same_f32(remap(a, b, c, d, e), hx(r[11]), "remap", &r);
        same_f32(step(a, b), hx(r[12]), "step", &r);
        same_f32(sign(a), hx(r[13]), "sign", &r);
        same_f32(clamp(a, b, c), hx(r[14]), "clamp", &r);
        same_f32(min(a, b), hx(r[15]), "min", &r);
        same_f32(max(a, b), hx(r[16]), "max", &r);
        same_f32(smoothstep(a, b, c), hx(r[17]), "smoothstep", &r);
        n += 1;
    }
    assert_eq!(n, 2048);
}
