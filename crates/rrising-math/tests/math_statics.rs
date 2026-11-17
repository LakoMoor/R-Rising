//! Parity tests for the transcribed `Unity.Mathematics.math` statics.
//!
//! Each test names the RVA of the `GameAssembly.dll` body the implementation came from, and the
//! expected values are read off that body (or off the shipped `.rdata`), not off the port. Where a
//! function's result is a bit pattern rather than a number — `hash`, `f32tof16`, `bitmask` — the
//! test pins the bits, because that is the observable thing.
// Expected values are the literals read from the original; keep them as written.
#![allow(clippy::approx_constant, clippy::excessive_precision)]

use rrising_math::*;

fn approx(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn v3_close(a: float3, b: float3, tol: f32) -> bool {
    approx(a.x, b.x, tol) && approx(a.y, b.y, tol) && approx(a.z, b.z, tol)
}

#[test]
fn masks_match_the_bool2_bodies() {
    // math.all_1225 @ 0x92b2c40, math.any_1210 @ 0x92b2a30.
    assert!(all(bool2::new(true, true)));
    assert!(!all(bool2::new(true, false)));
    assert!(!all(bool2::new(false, true)));
    assert!(any(bool2::new(false, true)));
    assert!(!any(bool2::new(false, false)));
    assert!(all(bool4::new(true, true, true, true)));
    assert!(!all(bool4::new(true, true, true, false)));
    assert!(any(bool3::new(false, false, true)));
    // The numeric forms test each component against zero.
    assert!(all(float3::new(1.0, 2.0, 3.0)));
    assert!(!all(float3::new(1.0, 0.0, 3.0)));
    assert!(any(int2::new(0, -4)));
    assert!(!any(uint4::zero));
}

#[test]
fn select_picks_b_when_the_flag_is_set() {
    // math.select_1240 @ 0x92b2e50: `if (c != 0) a = b; return a;`.
    assert_eq!(select(10.0f32, 20.0f32, true), 20.0);
    assert_eq!(select(10.0f32, 20.0f32, false), 10.0);
    assert_eq!(select(7i32, 9i32, false), 7);
}

#[test]
fn reductions_use_the_original_grouping() {
    // math.csum_1442 @ 0x92b9180, math.cmax_1430 @ 0x92b8ef0, math.cmin_1418 @ 0x92b8c60.
    assert_eq!(csum(float2::new(1.5, 2.25)), 3.75);
    assert_eq!(csum(float3::new(1.0, 2.0, 4.0)), 7.0);
    assert_eq!(csum(float4::new(1.0, 2.0, 4.0, 8.0)), 15.0);
    assert_eq!(csum(int4::new(-1, 2, -3, 4)), 2);
    assert_eq!(csum(uint2::new(3, 5)), 8u32);
    assert_eq!(cmax(float4::new(1.0, 9.0, 4.0, 2.0)), 9.0);
    assert_eq!(cmin(float4::new(1.0, 9.0, -4.0, 2.0)), -4.0);
    assert_eq!(cmax(int3::new(-5, -2, -9)), -2);
    assert_eq!(cmin(int3::new(-5, -2, -9)), -9);
    assert_eq!(cmax(uint4::new(1, 9, 4, 2)), 9u32);
    assert_eq!(cmin(uint4::new(1, 9, 4, 2)), 1u32);
    assert_eq!(csum(double4::new(1.0, 2.0, 4.0, 8.0)), 15.0);
    assert_eq!(cmax(double2::new(1.0, 2.0)), 2.0f64);

    // `cmax`/`cmin` are the body's compare-and-select, so the tie returns the same operand the
    // body returns: `cmax` takes `y` when `x <= y`, `cmin` takes `y` when `y <= x`.
    assert_eq!(cmax(float2::new(3.0, 3.0)), 3.0);
    // And the NaN rule is `math.min`/`math.max`'s, not Rust's: a NaN second operand is dropped.
    assert_eq!(cmax(float2::new(3.0, f32::NAN)), 3.0);
    assert_eq!(cmin(float2::new(3.0, f32::NAN)), 3.0);
}

#[test]
fn bit_reinterpretations_round_trip() {
    // math.asfloat_751 @ 0x92a4f00 (`return param_1;`), math.asint_732 @ 0x92a4e10.
    assert_eq!(asint(1.0), 0x3f800000u32 as i32);
    assert_eq!(asuint(1.0), 0x3f800000);
    assert_eq!(asfloat(0x3f800000u32 as i32), 1.0);
    assert_eq!(asfloat(asint(-2.5)), -2.5);
    assert_eq!(asuint(asfloat(0xdeadbeefu32 as i32)), 0xdeadbeef);
    assert_eq!(
        aslong(asdouble(0x1234_5678_9abc_def0u64 as i64)),
        0x1234_5678_9abc_def0u64 as i64
    );
    assert_eq!(asulong(3.25), asdouble(3.25f64.to_bits() as i64).to_bits());
}

#[test]
fn bit_tricks_match_their_bodies() {
    // countbits_1317 @ 0x92b5b80, lzcnt_1326 @ 0x92b61e0, tzcnt_1337 @ 0x92b6540,
    // reversebits_1346 @ 0x92b68b0, rol_1357 @ 0x92b71c0, ror_1367 @ 0x92b75d0.
    assert_eq!(countbits(0), 0);
    assert_eq!(countbits(-1), 32);
    assert_eq!(countbits(0x0f0f_0f0f), 16);
    assert_eq!(lzcnt(0), 32);
    assert_eq!(lzcnt(1), 31);
    assert_eq!(lzcnt(0x8000_0000u32 as i32), 0);
    assert_eq!(tzcnt(0), 32);
    assert_eq!(tzcnt(1), 0);
    assert_eq!(tzcnt(0x8000_0000u32 as i32), 31);
    assert_eq!(reversebits(1), 0x8000_0000u32 as i32);
    assert_eq!(reversebits(reversebits(0x1234_5678)), 0x1234_5678);
    assert_eq!(rol(1, 1), 2);
    assert_eq!(rol(1, 32), 1); // the body masks the shift with 0x1f
    assert_eq!(ror(2, 1), 1);
    assert_eq!(ror(1, 1), 0x8000_0000u32 as i32);

    // ispow2_785 @ 0x92a58e0, ceilpow2_1376 @ 0x4740e90, floorlog2_1394 @ 0x8e97ce0,
    // ceillog2_1386 @ 0x8e97d30.
    assert!(!ispow2(0));
    assert!(!ispow2(-1));
    assert!(ispow2(1));
    assert!(ispow2(1024));
    assert!(!ispow2(1023));
    assert_eq!(ceilpow2(0), 0); // `x - 1` wraps, the spread lands on 0xffffffff, +1 wraps back
    assert_eq!(ceilpow2(1), 1);
    assert_eq!(ceilpow2(5), 8);
    assert_eq!(ceilpow2(1024), 1024);
    assert_eq!(floorlog2(0), -1);
    assert_eq!(floorlog2(1), 0);
    assert_eq!(floorlog2(1023), 9);
    assert_eq!(floorlog2(1024), 10);
    // The body tests `x - 1U != 0`, so 0 does not take the early-out that 1 does.
    assert_eq!(ceillog2(1), 0);
    assert_eq!(ceillog2(2), 1);
    assert_eq!(ceillog2(3), 2);
    assert_eq!(ceillog2(1024), 10);
    assert_eq!(ceillog2(0), 32);

    // bitmask_758 @ 0x92a4f20, fold_to_uint_1480 @ 0x92baab0.
    assert_eq!(bitmask(bool4::new(false, false, false, false)), 0);
    assert_eq!(bitmask(bool4::new(true, false, false, false)), 1);
    assert_eq!(bitmask(bool4::new(false, true, false, true)), 0b1010);
    assert_eq!(bitmask(bool4::new(true, true, true, true)), 15);
    assert_eq!(fold_to_uint(1.0f64), 0x3ff0_0000); // bits 0x3FF00000_00000000 folded
    assert_eq!(
        fold_to_uint(asdouble(0x0123_4567_89ab_cdefi64)),
        0x0123_4567 ^ 0x89ab_cdef
    );
}

#[test]
fn compress_writes_the_masked_components_in_order() {
    // math.compress_1454 @ 0x19ad7c0.
    let mut out = [0.0f32; 4];
    let next = compress(
        &mut out,
        0,
        float4::new(10.0, 20.0, 30.0, 40.0),
        bool4::new(true, false, true, true),
    );
    assert_eq!(next, 3);
    assert_eq!(out, [10.0, 30.0, 40.0, 0.0]);
    let next = compress(
        &mut out,
        next,
        float4::new(1.0, 2.0, 3.0, 4.0),
        bool4::new(false, true, false, false),
    );
    assert_eq!(next, 4);
    assert_eq!(out, [10.0, 30.0, 40.0, 2.0]);
}

#[test]
fn bool_hashes_match_their_bodies_bit_for_bit() {
    // hash_3 @ 0x928c140 / hashwide_4 @ 0x928c1b0 (bool2), hash_33 @ 0x928d0a0 / hashwide_34 @
    // 0x928d0f0 (bool3), hash_67 @ 0x928e0d0 / hashwide_68 @ 0x928e130 (bool4). The values below
    // are the constants the bodies select between, summed in the order each body adds them.
    let h = hashwide(bool2::new(false, false));
    assert_eq!((h.x, h.y), (0xc9f27fcb, 0xcc74b3b7));
    let h = hashwide(bool2::new(true, true));
    assert_eq!((h.x, h.y), (0x6e2bf6a9, 0x6d2523b1));
    assert_eq!(hash(bool2::new(false, false)), 0xedbc_6790);
    assert_eq!(hash(bool2::new(true, false)), 0x57ca_d9b2);
    assert_eq!(hash(bool2::new(false, true)), 0xf2fe_cb78);
    assert_eq!(hash(bool2::new(true, true)), 0x5d0d_3d9a);

    let h = hashwide3(bool3::new(true, false, true));
    assert_eq!((h.x, h.y, h.z), (0xae131389, 0x670d6809, 0x87096cd7));
    for m in 0..8u32 {
        let v = bool3::new(m & 1 != 0, m & 2 != 0, m & 4 != 0);
        let expected = [
            0x7ca3_962f,
            0x8a3b_cf0b,
            0xd218_a45b,
            0xdfb0_dd37,
            0xcacc_fc63,
            0xd865_353f,
            0x2042_0a8f,
            0x2dda_436b,
        ][m as usize];
        assert_eq!(hash3(v), expected, "bool3 mask {m:b}");
    }

    let h = hashwide4(bool4::new(true, true, true, true));
    assert_eq!(
        (h.x, h.y, h.z, h.w),
        (0xb40ec62d, 0x565ed63f, 0x93c30c2b, 0xdcaf0351)
    );
    for m in 0..16u32 {
        let v = bool4::new(m & 1 != 0, m & 2 != 0, m & 4 != 0, m & 8 != 0);
        let expected = [
            0x6ea1_81ae,
            0xd3e1_5dea,
            0x6388_7ec0,
            0xc8c8_5afc,
            0x42ac_c3a0,
            0xa7ec_9fdc,
            0x3793_c0b2,
            0x9cd3_9cee,
            0x6690_41dc,
            0xcbd0_1e18,
            0x5b77_3eee,
            0xc0b7_1b2a,
            0x3a9b_83ce,
            0x9fdb_600a,
            0x2f82_80e0,
            0x94c2_5d1c,
        ][m as usize];
        assert_eq!(hash4(v), expected, "bool4 mask {m:04b}");
    }
}

#[test]
fn vector_geometry_matches_the_bodies() {
    // mad_859 @ 0x92a7560 (`z + x*y`), mad3 is the component-wise form.
    assert_eq!(mad(2.0, 3.0, 10.0), 16.0);
    let m = mad3(
        float3::new(1.0, 2.0, 3.0),
        float3::new(4.0, 5.0, 6.0),
        float3::new(10.0, 20.0, 30.0),
    );
    assert!(v3_close(m, float3::new(14.0, 30.0, 48.0), 0.0));

    // distancesq_1192 @ 0x92b1d70.
    assert_eq!(
        distancesq(float3::new(1.0, 2.0, 3.0), float3::new(4.0, 6.0, 3.0)),
        9.0 + 16.0
    );

    // faceforward_1302 @ 0x92b4df0: negate N when dot(Nref, I) >= 0.
    let n = float3::new(0.0, 1.0, 0.0);
    assert!(v3_close(
        faceforward(n, float3::new(0.0, -1.0, 0.0), n),
        n,
        0.0
    ));
    assert!(v3_close(
        faceforward(n, float3::new(0.0, 1.0, 0.0), n),
        -n,
        0.0
    ));

    // reflect_1278 @ 0x92b35a0: I - (N + N) * dot(N, I).
    let r = reflect(float3::new(1.0, -1.0, 0.0), float3::new(0.0, 1.0, 0.0));
    assert!(v3_close(r, float3::new(1.0, 1.0, 0.0), 1e-6));

    // refract_1284 @ 0x92b3960, with the body's total-internal-reflection zero.
    let f = refract(float3::new(0.0, -1.0, 0.0), float3::new(0.0, 1.0, 0.0), 1.0);
    assert!(v3_close(f, float3::new(0.0, -1.0, 0.0), 1e-6));
    let t = refract(
        float3::new(0.7071068, -0.7071068, 0.0),
        float3::new(0.0, 1.0, 0.0),
        2.0,
    );
    assert!(
        v3_close(t, float3::zero, 0.0),
        "expected the zero fallback, got {t}"
    );

    // project_1290 @ 0x92b4170 and projectsafe_1293 @ 0x92b4390.
    let p = project(float3::new(3.0, 4.0, 0.0), float3::new(1.0, 0.0, 0.0));
    assert!(v3_close(p, float3::new(3.0, 0.0, 0.0), 1e-6));
    let d = float3::new(9.0, 9.0, 9.0);
    assert!(v3_close(
        projectsafe(float3::new(1.0, 0.0, 0.0), float3::zero, d),
        d,
        0.0
    ));
    assert!(v3_close(
        projectsafe(float3::new(1.0, 2.0, 3.0), float3::new(1.0, 0.0, 0.0), d),
        float3::new(1.0, 0.0, 0.0),
        1e-6
    ));

    // normalizesafe_1162 @ 0x92b1510: the guard is `1.17549435e-38 < lengthsq(value)`.
    let ns = normalizesafe(float3::new(0.0, 3.0, 4.0), d);
    assert!(v3_close(ns, float3::new(0.0, 0.6, 0.8), 1e-6));
    assert!(v3_close(normalizesafe(float3::zero, d), d, 0.0));
    // A vector just under the threshold still falls back.
    assert!(v3_close(normalizesafe(float3::splat(1e-20), d), d, 0.0));
}

#[test]
fn unit_vectors_match_the_stores_in_their_bodies() {
    // up @ 0x92ba140, down @ 0x92ba150, forward @ 0x92ba170, back @ 0x92ba180, left @ 0x92ba190,
    // right @ 0x92ba1a0. Each body writes 0x3f800000 or 0xbf800000 into exactly one component.
    assert!(v3_close(up(), float3::new(0.0, 1.0, 0.0), 0.0));
    assert!(v3_close(down(), float3::new(0.0, -1.0, 0.0), 0.0));
    assert!(v3_close(forward(), float3::new(0.0, 0.0, 1.0), 0.0));
    assert!(v3_close(back(), float3::new(0.0, 0.0, -1.0), 0.0));
    assert!(v3_close(left(), float3::new(-1.0, 0.0, 0.0), 0.0));
    assert!(v3_close(right(), float3::new(1.0, 0.0, 0.0), 0.0));
}

#[test]
fn interpolation_and_transcendentals_match_their_bodies() {
    // unlerp_843 @ 0x92a6a90, remap_851 @ 0x92a6e40.
    assert_eq!(unlerp(2.0, 4.0, 3.0), 0.5);
    assert_eq!(unlerp(2.0, 4.0, 2.0), 0.0);
    assert_eq!(remap(0.0, 1.0, 10.0, 20.0, 0.25), 12.5);
    assert_eq!(remap(2.0, 4.0, -1.0, 1.0, 3.0), 0.0);

    // modf_1132 @ 0x92b0430: the out parameter gets trunc(x), the result is x - trunc(x).
    let mut i = 0.0;
    assert_eq!(modf(-2.75, &mut i), -0.75);
    assert_eq!(i, -2.0);
    let mut i = 0.0;
    assert_eq!(modf(7.25, &mut i), 0.25);
    assert_eq!(i, 7.0);

    // sincos_1308 @ 0x92b51f0.
    let mut s = 0.0;
    let mut c = 0.0;
    sincos(0.0, &mut s, &mut c);
    assert_eq!((s, c), (0.0, 1.0));
    sincos(std::f32::consts::FRAC_PI_2, &mut s, &mut c);
    assert!(approx(s, 1.0, 1e-6) && approx(c, 0.0, 1e-6));

    // exp2_1084 @ 0x92aecb0 and exp10_1092 @ 0x92af010 scale by ln2 / ln10 in float, then take the
    // double exp and truncate back.
    assert!(approx(exp2(3.0), 8.0, 1e-5));
    assert!(approx(exp2(0.0), 1.0, 0.0));
    assert!(approx(exp10(2.0), 100.0, 1e-4));
    assert!(approx(exp10(0.0), 1.0, 0.0));
}

#[test]
fn quaternion_exp_log_and_nlerp_match_their_bodies() {
    // unitexp_1698 @ 0x92cc850 / unitlog_1700 @ 0x92cca20 are inverses on a pure quaternion.
    let axis = normalize(float3::new(0.3, -0.6, 0.75));
    let angle = 0.9f32;
    let pure = quaternion::new(axis.x * angle, axis.y * angle, axis.z * angle, 0.0);
    let e = unitexp(pure);
    assert!(
        approx(
            e.value.x * e.value.x
                + e.value.y * e.value.y
                + e.value.z * e.value.z
                + e.value.w * e.value.w,
            1.0,
            1e-5
        ),
        "unitexp must produce a unit quaternion: {e:?}"
    );
    let back = unitlog(e);
    assert!(
        approx(back.value.x, pure.value.x, 1e-4)
            && approx(back.value.y, pure.value.y, 1e-4)
            && approx(back.value.z, pure.value.z, 1e-4),
        "unitlog(unitexp) drifted: {back:?} vs {pure:?}"
    );
    assert_eq!(back.value.w, 0.0);

    // unitlog clamps w into [-1, 1] (the -1.0f at 0x18cf29990), but the shipped body has no guard
    // for a clamp that lands on 1.0: `1.0f / sqrt(1 - 1*1)` is inf, `inf * acos(1.0f)` is NaN.
    let l = unitlog(quaternion::new(0.0, 0.0, 2.5, 1.4));
    assert!(
        l.value.x.is_nan() && l.value.z.is_nan() && l.value.w == 0.0,
        "{l:?}"
    );

    // nlerp_1705 @ 0x92ccdb0: endpoints are the inputs (normalized), and the antipodal case takes
    // the short way round because the body flips `b` when the dot product is negative.
    let a = quaternion::rotate_z(0.4);
    let b = quaternion::rotate_z(1.2);
    assert!(
        approx(nlerp(a, b, 0.0).value.z, a.value.z, 1e-6),
        "nlerp t=0 must return a"
    );
    assert!(
        approx(nlerp(a, b, 1.0).value.z, b.value.z, 1e-6),
        "nlerp t=1 must return b"
    );
    let mid = nlerp(a, b, 0.5);
    assert!(
        approx(mid.value.z, quaternion::rotate_z(0.8).value.z, 1e-5),
        "{mid:?}"
    );
    let flipped = nlerp(
        a,
        quaternion::new(-a.value.x, -a.value.y, -a.value.z, -a.value.w),
        1.0,
    );
    assert!(
        approx(flipped.value.w, a.value.w, 1e-6) && approx(flipped.value.z, a.value.z, 1e-6),
        "nlerp must flip the antipodal target, not rotate the long way: {flipped:?} vs {a:?}"
    );
}

#[test]
fn orthonormalize_is_the_bodies_gram_schmidt_not_the_textbook_one() {
    // orthonormalize_1489 @ 0x92bae80. The third input column is ignored, the second is projected
    // against the unnormalized first, and the third output is the cross product of the two.
    let m = float3x3::new(
        float3::new(2.0, 0.0, 0.0),
        float3::new(1.0, 3.0, 0.0),
        float3::new(7.0, 7.0, 7.0),
    );
    let o = orthonormalize(m);
    // dot(c1, c0) = 2, so v = (1,3,0) - (2,0,0) * 2 = (-3,3,0) and |v| = 3*sqrt(2); c2 is ignored.
    assert!(v3_close(o.c0, float3::new(1.0, 0.0, 0.0), 1e-6), "{o:?}");
    assert!(
        v3_close(o.c1, float3::new(-0.7071068, 0.7071068, 0.0), 1e-6),
        "{o:?}"
    );
    assert!(
        v3_close(o.c2, float3::new(0.0, 0.0, 0.7071068), 1e-6),
        "{o:?}"
    );

    // Because the projection subtracts `c0 * dot(c1, c0)` instead of the projection onto the unit
    // c0, the output is only orthonormal when c0 arrives unit length. With a unit c0 the same
    // shape of input does give an orthonormal basis.
    let s = orthonormalize(float3x3::new(
        float3::new(0.0, 1.0, 0.0),
        float3::new(2.0, 3.0, 4.0),
        float3::new(5.0, 6.0, 7.0),
    ));
    for (c, name) in [(s.c0, "c0"), (s.c1, "c1"), (s.c2, "c2")] {
        assert!(
            approx(c.lengthsq(), 1.0, 1e-5),
            "{name} is not unit length: {c}"
        );
    }
    assert!(
        approx(s.c0.dot(s.c1), 0.0, 1e-5),
        "c0.c1 {}",
        s.c0.dot(s.c1)
    );
    assert!(
        approx(s.c0.dot(s.c2), 0.0, 1e-5),
        "c0.c2 {}",
        s.c0.dot(s.c2)
    );
    assert!(
        approx(s.c1.dot(s.c2), 0.0, 1e-5),
        "c1.c2 {}",
        s.c1.dot(s.c2)
    );

    // The degenerate fallback the body takes when either length is <= 1e-30 (0x18cf28590): the
    // canonical basis, not NaN.
    let d = orthonormalize(float3x3::new(float3::zero, float3::zero, float3::zero));
    assert!(v3_close(d.c0, float3::new(1.0, 0.0, 0.0), 0.0), "{d:?}");
    assert!(v3_close(d.c1, float3::new(0.0, 1.0, 0.0), 0.0), "{d:?}");
    assert!(v3_close(d.c2, float3::new(0.0, 0.0, 1.0), 0.0), "{d:?}");
}

#[test]
fn shuffles_use_the_component_indices_the_interleave_bodies_pass() {
    // select_shuffle_component_9 @ 0x928c370 switches 0-3 on `left` and 4-7 on `right`.
    let a = float4::new(0.0, 1.0, 2.0, 3.0);
    let b = float4::new(10.0, 11.0, 12.0, 13.0);
    assert_eq!(select_shuffle_component(a, b, ShuffleComponent::LeftW), 3.0);
    assert_eq!(
        select_shuffle_component(a, b, ShuffleComponent::RightX),
        10.0
    );
    assert_eq!(
        shuffle(
            a,
            b,
            ShuffleComponent::RightW,
            ShuffleComponent::LeftX,
            ShuffleComponent::RightZ,
            ShuffleComponent::LeftZ
        ),
        float4::new(13.0, 0.0, 12.0, 2.0)
    );

    // movelh_1476 -> 0,1,4,5; movehl_1478 -> 2,3,6,7; unpacklo_1472 -> 0,4,1,5; unpackhi_1474 ->
    // 2,6,3,7. These are the indices the four bodies pass to select_shuffle_component.
    assert_eq!(movelh(a, b), float4::new(0.0, 1.0, 10.0, 11.0));
    assert_eq!(movehl(a, b), float4::new(2.0, 3.0, 12.0, 13.0));
    assert_eq!(unpacklo(a, b), float4::new(0.0, 10.0, 1.0, 11.0));
    assert_eq!(unpackhi(a, b), float4::new(2.0, 12.0, 3.0, 13.0));
}

#[test]
fn half_conversions_round_trip_every_finite_half() {
    // f16tof32_1457 @ 0x612f520, f32tof16_1461 @ 0x92b9520. Feeding every finite binary16 pattern
    // through f16tof32 and back must reproduce it exactly; that is the strongest check available
    // without running the original.
    for bits in 0u32..0x1_0000 {
        if (bits >> 10) & 0x1f == 0x1f {
            continue; // Inf and NaN are checked separately below.
        }
        let v = f16tof32(bits as i32);
        assert_eq!(
            f32tof16(v),
            bits,
            "half {bits:04x} -> {v} -> {:04x}",
            f32tof16(v)
        );
    }

    // The constants the bodies read out of .rdata: 6.103515625e-05f at 0x18cf28614 is the subnormal
    // fix-up, so the smallest subnormal half must survive the round trip.
    assert_eq!(f16tof32(1), 5.9604645e-8);
    assert_eq!(f32tof16(5.9604645e-8), 1);
    assert_eq!(f16tof32(0x3c00), 1.0);
    assert_eq!(f16tof32(0xbc00), -1.0);
    assert_eq!(f32tof16(-0.0), 0x8000);
    assert_eq!(f16tof32(0x8000), -0.0);
    assert_eq!(f16tof32(0x7bff), 65504.0); // the largest finite half
    assert_eq!(f32tof16(65504.0), 0x7bff);

    // Inf and NaN come from the magnitude test the body performs on the masked bits: exactly
    // 0x7F800000 is Inf (0x7c00), anything above is NaN (0x7e00).
    assert_eq!(f32tof16(f32::INFINITY), 0x7c00);
    assert_eq!(f32tof16(f32::NEG_INFINITY), 0xfc00);
    assert_eq!(f32tof16(f32::NAN), 0x7e00);
    assert!(f16tof32(0x7c00).is_infinite());
    assert!(f16tof32(0x7e00).is_nan());

    // Overflow is not clamped to 0x7c00: the body's clamp constant (260042752.0f at 0x18cf29590)
    // only catches Inf/NaN, so a finite float above the half range yields a 17-bit value.
    assert_eq!(f32tof16(70000.0), 0x7c46);
    assert_eq!(f32tof16(1e30), 0x1ca50);
}
