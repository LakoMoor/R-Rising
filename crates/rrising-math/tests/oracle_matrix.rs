//! Differential tests against Unity.Mathematics float3x3 / float4x4 / quaternion leaves replayed from
//! the original GameAssembly.dll (x86-64 under Rosetta). Golden rows come from
//! `tools/capture_math_matrix.py`; only inputs and outputs are stored, compared bit for bit.
use rrising_math::*;

fn hx(s: &str) -> f32 {
    f32::from_bits(u32::from_str_radix(s, 16).unwrap())
}

fn same(got: &[f32], want: &[f32]) -> bool {
    got.len() == want.len()
        && got
            .iter()
            .zip(want)
            .all(|(g, w)| g.to_bits() == w.to_bits() || (g.is_nan() && w.is_nan()))
}

fn m4(v: &[f32]) -> float4x4 {
    float4x4::new(
        float4::new(v[0], v[1], v[2], v[3]),
        float4::new(v[4], v[5], v[6], v[7]),
        float4::new(v[8], v[9], v[10], v[11]),
        float4::new(v[12], v[13], v[14], v[15]),
    )
}

fn flat(m: float4x4) -> [f32; 16] {
    [
        m.c0.x, m.c0.y, m.c0.z, m.c0.w, m.c1.x, m.c1.y, m.c1.z, m.c1.w, m.c2.x, m.c2.y, m.c2.z,
        m.c2.w, m.c3.x, m.c3.y, m.c3.z, m.c3.w,
    ]
}

#[test]
fn matrix_and_quaternion_match_original() {
    let src = include_str!("../../../evidence/oracles/math_matrix.tsv");
    let names = [
        "float3x3(quaternion)",
        "float4x4(quaternion, float3)",
        "float4x4.TRS",
        "float4x4.Translate",
        "float4x4.Scale(float3)",
        "Translate*(Rotate*Scale)",
        "mul(float4x4, float4x4)",
        "normalize(quaternion)",
        "mul(quaternion, quaternion)",
    ];
    let mut bad = [0usize; 9];
    let mut first: [Option<String>; 9] = Default::default();
    let mut rows = 0;
    for line in src.lines().filter(|l| !l.starts_with('#') && !l.is_empty()) {
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 161);
        let v: Vec<f32> = f
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != 47)
            .map(|(_, s)| hx(s))
            .collect();
        let t = float3::new(v[0], v[1], v[2]);
        let q = quaternion::new(v[4], v[5], v[6], v[7]);
        let q2 = quaternion::new(v[8], v[9], v[10], v[11]);
        let s = float3::new(v[12], v[13], v[14]);
        let a = m4(&v[15..31]);
        let b = m4(&v[31..47]);
        let o = &v[47..];
        let m3 = float3x3::from_quaternion(q);
        let got: [Vec<f32>; 9] = [
            vec![
                m3.c0.x, m3.c0.y, m3.c0.z, m3.c1.x, m3.c1.y, m3.c1.z, m3.c2.x, m3.c2.y, m3.c2.z,
            ],
            {
                let mut r = float4x4::rotate(q);
                r.c3 = float4::new(t.x, t.y, t.z, 1.0);
                flat(r).to_vec()
            },
            flat(float4x4::trs(t, q, s)).to_vec(),
            flat(float4x4::translate(t)).to_vec(),
            flat(float4x4::scale(s)).to_vec(),
            flat(float4x4::mul_matrix(
                float4x4::translate(t),
                float4x4::mul_matrix(float4x4::rotate(q), float4x4::scale(s)),
            ))
            .to_vec(),
            flat(float4x4::mul_matrix(a, b)).to_vec(),
            {
                let n = quaternion::normalize(q).value;
                vec![n.x, n.y, n.z, n.w]
            },
            {
                let m = (q * q2).value;
                vec![m.x, m.y, m.z, m.w]
            },
        ];
        let mut at = 0;
        for (k, g) in got.iter().enumerate() {
            let w = &o[at..at + g.len()];
            at += g.len();
            if !same(g, w) {
                bad[k] += 1;
                first[k].get_or_insert_with(|| line.to_string());
            }
        }
        rows += 1;
    }
    assert_eq!(rows, 2048);
    let mut fail = String::new();
    for k in 0..9 {
        if bad[k] != 0 {
            fail += &format!("{}: {} of {rows} differ\n", names[k], bad[k]);
        }
    }
    assert!(fail.is_empty(), "{fail}");
}
