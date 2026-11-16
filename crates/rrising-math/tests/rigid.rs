//! Pins for `RigidTransform`, the matrix→quaternion conversions and `math.fastinverse`.
//!
//! Every expectation here comes from a decompiled body in
//! `evidence/decompiled/Unity.Mathematics.dll.c` (the RVAs are in the source comments), not from
//! what a textbook says the result should be. Where the body does something surprising — negating a
//! quaternion component by multiplying by −1.0f, summing a squared length `y, x, z` — the test pins
//! the surprise.

use rrising_math::{
    float3, float3x3, float4, float4x4, hash_rigid, hashwide_rigid, quaternion, rigid_transform,
    rigid_transform_m3, rigid_transform_m4, RigidTransform, RotationOrder,
};
use rrising_math::math::{fastinverse, normalize};

fn close(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol || (a.is_nan() && b.is_nan())
}

fn v3_close(a: float3, b: float3, tol: f32) -> bool {
    close(a.x, b.x, tol) && close(a.y, b.y, tol) && close(a.z, b.z, tol)
}

fn q_close(a: quaternion, b: quaternion, tol: f32) -> bool {
    close(a.value.x, b.value.x, tol)
        && close(a.value.y, b.value.y, tol)
        && close(a.value.z, b.value.z, tol)
        && close(a.value.w, b.value.w, tol)
}

fn m3_close(a: float3x3, b: float3x3, tol: f32) -> bool {
    v3_close(a.c0, b.c0, tol) && v3_close(a.c1, b.c1, tol) && v3_close(a.c2, b.c2, tol)
}

fn m4_close(a: float4x4, b: float4x4, tol: f32) -> bool {
    [a.c0, a.c1, a.c2, a.c3]
        .iter()
        .zip([b.c0, b.c1, b.c2, b.c3].iter())
        .all(|(x, y)| {
            close(x.x, y.x, tol)
                && close(x.y, y.y, tol)
                && close(x.z, y.z, tol)
                && close(x.w, y.w, tol)
        })
}

fn rt_close(a: RigidTransform, b: RigidTransform, tol: f32) -> bool {
    q_close(a.rot, b.rot, tol) && v3_close(a.pos, b.pos, tol)
}

/// Rotations worth testing: the identity, a single axis, an Euler triple, an axis-angle and a
/// 90°-about-Z basis (whose diagonal has a negative entry, which is what exercises the branchless
/// conversion's sign masks).
fn samples() -> Vec<quaternion> {
    vec![
        quaternion::identity,
        quaternion::rotate_x(0.7),
        quaternion::rotate_y(-1.9),
        quaternion::rotate_z(std::f32::consts::FRAC_PI_2),
        quaternion::euler_zxy(float3::new(0.3, -0.9, 1.2)),
        quaternion::euler_xyz(float3::new(2.0, 0.25, -0.75)),
        quaternion::axis_angle(normalize(float3::new(0.3, -0.6, 0.75)), 1.1),
        quaternion::look_rotation_safe(float3::new(0.3, -0.6, 0.75), float3::new(-0.2, 0.9, 0.1)),
    ]
}

#[test]
fn matrix_to_quaternion_inverts_from_quaternion() {
    // quaternion(float3x3) @ 0x93757a0 and quaternion(float4x4) @ 0x9375aa0 are right inverses of
    // float3x3(quaternion) on orthonormal inputs, and the two shapes agree with each other.
    for q in samples() {
        let m = float3x3::from_quaternion(q);
        let back = quaternion::from_matrix3(m);
        assert!(
            m3_close(float3x3::from_quaternion(back), m, 1e-5),
            "float3x3 round trip lost the rotation: {q:?} -> {back:?}"
        );
        // The 4×4 form reads the same nine elements, so it must give the same quaternion.
        let back4 = quaternion::from_matrix4(float4x4::rotate(q));
        assert!(
            m3_close(float3x3::from_quaternion(back4), m, 1e-5),
            "float4x4 conversion disagrees with float3x3: {q:?}"
        );
        // Columns stay unit length: the body ends by dividing by the length.
        assert!(close(m.c0.dot(m.c0), 1.0, 1e-5));
        assert!(close(m.c1.dot(m.c1), 1.0, 1e-5));
        assert!(close(m.c2.dot(m.c2), 1.0, 1e-5));
    }
    // The identity matrix converts to the identity quaternion exactly: the shuffles move the
    // `1.0 + |m00|` term into `w` and the rest stay zero.
    assert_eq!(quaternion::from_matrix3(float3x3::identity), quaternion::identity);
    assert_eq!(quaternion::from_matrix4(float4x4::identity), quaternion::identity);
}

#[test]
fn look_rotation_safe_builds_the_bodies_basis() {
    // float3x3.LookRotationSafe @ 0x9339fc0: c2 = normalize(forward),
    // c0 = normalize(cross(up, forward)), c1 = cross(c2, c0).
    let up = float3::new(0.0, 1.0, 0.0);
    assert_eq!(
        float3x3::look_rotation_safe(float3::new(0.0, 0.0, 1.0), up),
        float3x3::identity
    );

    // Looking along +Y with +Z as up: right = cross((0,0,1), (0,1,0)) = (-1, 0, 0).
    let m = float3x3::look_rotation_safe(float3::new(0.0, 1.0, 0.0), float3::new(0.0, 0.0, 1.0));
    assert!(v3_close(m.c0, float3::new(-1.0, 0.0, 0.0), 0.0));
    assert!(v3_close(m.c1, float3::new(0.0, 0.0, 1.0), 0.0));
    assert!(v3_close(m.c2, float3::new(0.0, 1.0, 0.0), 0.0));

    // A generic pair: orthonormal, forward along +Z of the basis, right handed with the body's
    // `cross(up, forward)` choice.
    let f = float3::new(0.3, -0.6, 0.75);
    let u = float3::new(-0.2, 0.9, 0.1);
    let g = float3x3::look_rotation_safe(f, u);
    assert!(v3_close(g.c2, normalize(f), 1e-6));
    assert!(v3_close(g.c0, normalize(u.cross(f)), 1e-6));
    assert!(v3_close(g.c1, g.c2.cross(g.c0), 1e-6));
    assert!(close(g.c0.dot(g.c1), 0.0, 1e-6));
    assert!(close(g.c1.dot(g.c2), 0.0, 1e-6));
    assert!(close(g.c2.dot(g.c0), 0.0, 1e-6));
}

#[test]
fn look_rotation_safe_falls_back_to_identity_when_degenerate() {
    // The fold over the three squared lengths rejects a zero or infinite input, and `up` parallel
    // to `forward` is rejected because the cross product collapses.
    let up = float3::new(0.0, 1.0, 0.0);
    let f = float3::new(0.3, -0.6, 0.75);
    let degenerate = [
        (float3::zero, up),
        (float3::splat(f32::INFINITY), up),
        (float3::splat(1e-20), up),
        (float3::splat(f32::NAN), up),
        (f, f),
    ];
    for (forward, upv) in degenerate {
        assert_eq!(
            float3x3::look_rotation_safe(forward, upv),
            float3x3::identity,
            "{forward:?} / {upv:?} must give the identity matrix"
        );
        assert_eq!(
            quaternion::look_rotation_safe(forward, upv),
            quaternion::identity,
            "{forward:?} / {upv:?} must give the identity quaternion"
        );
    }
    // Without the fallback the degenerate basis is already NaN (1.0 / sqrt(0)), so the identity
    // arm is what keeps a NaN out of a transform.
    let nan_basis = float3x3::new(
        float3::splat(f32::NAN),
        float3::splat(f32::NAN),
        float3::splat(f32::NAN),
    );
    assert!(quaternion::from_matrix3(nan_basis).value.x.is_nan());
}

#[test]
fn quaternion_look_rotation_safe_agrees_with_the_matrix() {
    // quaternion.LookRotationSafe @ 0x93761d0 builds the same 3×3 on the stack and hands it to
    // quaternion(float3x3).
    let pairs = [
        (float3::new(0.0, 0.0, 1.0), float3::new(0.0, 1.0, 0.0)),
        (float3::new(0.0, 1.0, 0.0), float3::new(0.0, 0.0, 1.0)),
        (float3::new(0.3, -0.6, 0.75), float3::new(-0.2, 0.9, 0.1)),
        (float3::new(-1.5, 0.25, 3.0), float3::new(0.0, -1.0, 0.0)),
    ];
    for (f, u) in pairs {
        let q = quaternion::look_rotation_safe(f, u);
        assert!(
            m3_close(float3x3::from_quaternion(q), float3x3::look_rotation_safe(f, u), 1e-5),
            "{f:?} / {u:?}: the quaternion and the matrix disagree"
        );
        // The quaternion is the local→world rotation whose +Z axis is `forward` — that is what
        // `LookRotation` means, and it is the basis' third column. Rotating `forward` itself by it
        // would apply the rotation twice.
        let z = quaternion::mul_qv(q, float3::new(0.0, 0.0, 1.0));
        assert!(v3_close(z, normalize(f), 1e-5), "{f:?}: +Z should map onto forward");
    }
}

#[test]
fn rigid_eulers_are_quaternion_eulers_with_a_zero_translation() {
    // RigidTransform.EulerXYZ…EulerZYX @ 0x938da30-0x938dd00 and the scalar overloads
    // @ 0x938dd90-0x938e100: each calls the matching quaternion.Euler* and leaves pos at zero.
    let v = float3::new(0.3, -0.9, 1.2);
    let by_order: [(RigidTransform, quaternion); 6] = [
        (RigidTransform::euler_xyz(v), quaternion::euler_xyz(v)),
        (RigidTransform::euler_xzy(v), quaternion::euler_xzy(v)),
        (RigidTransform::euler_yxz(v), quaternion::euler_yxz(v)),
        (RigidTransform::euler_yzx(v), quaternion::euler_yzx(v)),
        (RigidTransform::euler_zxy(v), quaternion::euler_zxy(v)),
        (RigidTransform::euler_zyx(v), quaternion::euler_zyx(v)),
    ];
    for (r, q) in by_order {
        assert!(q_close(r.rot, q, 0.0), "{q:?} disagrees with the RigidTransform form");
        assert_eq!(r.pos, float3::zero);
    }
    // The scalar overloads take the same arguments in the same order.
    for (r, q) in [
        (RigidTransform::euler_xyz_3(0.3, -0.9, 1.2), quaternion::euler_xyz(v)),
        (RigidTransform::euler_zxy_3(0.3, -0.9, 1.2), quaternion::euler_zxy(v)),
        (RigidTransform::euler_zyx_3(0.3, -0.9, 1.2), quaternion::euler_zyx(v)),
    ] {
        assert!(q_close(r.rot, q, 0.0));
    }
    // Euler(float3, RotationOrder) @ 0x938e1b0 dispatches over the shipped enum, whose 4 is ZXY.
    for (order, q) in [
        (RotationOrder::XYZ, quaternion::euler_xyz(v)),
        (RotationOrder::XZY, quaternion::euler_xzy(v)),
        (RotationOrder::YXZ, quaternion::euler_yxz(v)),
        (RotationOrder::YZX, quaternion::euler_yzx(v)),
        (RotationOrder::ZXY, quaternion::euler_zxy(v)),
        (RotationOrder::ZYX, quaternion::euler_zyx(v)),
    ] {
        assert!(q_close(RigidTransform::euler(v, order).rot, q, 0.0), "{order:?}");
        assert!(q_close(RigidTransform::euler_3(0.3, -0.9, 1.2, order).rot, q, 0.0));
    }
    // AxisAngle @ 0x938d960 and RotateX/Y/Z @ 0x938e500-0x938e660 likewise keep pos at zero, and
    // Translate @ 0x938e710 keeps the rotation at the identity.
    let axis = normalize(float3::new(0.3, -0.6, 0.75));
    assert!(q_close(RigidTransform::axis_angle(axis, 1.1).rot, quaternion::axis_angle(axis, 1.1), 0.0));
    assert_eq!(RigidTransform::axis_angle(axis, 1.1).pos, float3::zero);
    for (r, q) in [
        (RigidTransform::rotate_x(0.4), quaternion::rotate_x(0.4)),
        (RigidTransform::rotate_y(0.4), quaternion::rotate_y(0.4)),
        (RigidTransform::rotate_z(0.4), quaternion::rotate_z(0.4)),
    ] {
        assert!(q_close(r.rot, q, 0.0));
        assert_eq!(r.pos, float3::zero);
    }
    assert_eq!(
        RigidTransform::translate(float3::new(1.0, 2.0, 3.0)),
        RigidTransform::new(quaternion::identity, float3::new(1.0, 2.0, 3.0))
    );
    // identity @ 0x938f040 is the (0,0,0,1) literal at VA 0x18cf2a5d0 over a zero translation.
    assert_eq!(RigidTransform::identity.pos, float3::zero);
    assert_eq!(RigidTransform::identity.rot, quaternion::identity);
}

#[test]
fn rigid_inverse_keeps_the_bodies_negation_and_lane_order() {
    // math.inverse(RigidTransform) @ 0x92cd480 negates the vector part by *multiplying by −1.0f*,
    // so a zero component comes out as −0.0 rather than +0.0, and the reciprocal is
    // 1.0f / (y² + x² + z² + w²).
    let inv = RigidTransform::inverse(RigidTransform::identity);
    assert_eq!(inv.rot.value.x.to_bits(), (-0.0f32).to_bits());
    assert_eq!(inv.rot.value.y.to_bits(), (-0.0f32).to_bits());
    assert_eq!(inv.rot.value.z.to_bits(), (-0.0f32).to_bits());
    assert_eq!(inv.rot.value.w.to_bits(), 1.0f32.to_bits());
    assert_eq!(inv.pos, float3::zero);

    // The fused translation is `-(R⁻¹ · pos)`, checked against the two-step form.
    let t = RigidTransform::new(quaternion::euler_zxy(float3::new(0.3, -0.9, 1.2)), float3::new(4.0, -5.5, 6.0));
    let i = RigidTransform::inverse(t);
    let expect = -quaternion::mul_qv(i.rot, t.pos);
    assert!(v3_close(i.pos, expect, 1e-4), "{} vs {}", i.pos, expect);
    // Composing a transform with its inverse gives the identity.
    assert!(rt_close(RigidTransform::mul(i, t), RigidTransform::identity, 1e-4));
    assert!(rt_close(RigidTransform::mul(t, i), RigidTransform::identity, 1e-4));
}

#[test]
fn rigid_mul_composes_rotations_and_translations() {
    let a = RigidTransform::new(quaternion::rotate_y(0.6), float3::new(1.0, 2.0, 3.0));
    let b = RigidTransform::new(quaternion::rotate_x(-1.25), float3::new(-4.0, 0.5, 6.0));
    let ab = RigidTransform::mul(a, b);

    // The rotation is the quaternion product, checked through the matrix it builds.
    let expect = float3x3::mul_matrix(
        float3x3::from_quaternion(a.rot),
        float3x3::from_quaternion(b.rot),
    );
    assert!(
        m3_close(float3x3::from_quaternion(ab.rot), expect, 1e-5),
        "the fused quaternion product disagrees with the matrix product"
    );
    // The translation is `rotate(a.rot, b.pos) + a.pos`.
    assert!(
        v3_close(ab.pos, quaternion::mul_qv(a.rot, b.pos) + a.pos, 1e-5),
        "{} vs {}",
        ab.pos,
        quaternion::mul_qv(a.rot, b.pos) + a.pos
    );
    // Identity on either side, and associativity of composition.
    assert!(rt_close(RigidTransform::mul(RigidTransform::identity, b), b, 0.0));
    assert!(rt_close(RigidTransform::mul(a, RigidTransform::identity), a, 0.0));
    let c = RigidTransform::new(quaternion::rotate_z(2.0), float3::new(0.25, -0.5, 0.75));
    assert!(
        rt_close(
            RigidTransform::mul(RigidTransform::mul(a, b), c),
            RigidTransform::mul(a, RigidTransform::mul(b, c)),
            1e-3
        ),
        "composition is not associative to within f32 rounding"
    );
    // `mul(a, b)` puts `a` on the left of the quaternion product and applies `a` to `b.pos`, so
    // applying the composition means applying `b` first and then `a`.
    let p = float3::new(7.0, -8.0, 9.5);
    assert!(
        v3_close(
            RigidTransform::transform(ab, p),
            RigidTransform::transform(a, RigidTransform::transform(b, p)),
            1e-3
        ),
        "mul and transform disagree"
    );
    // `*` is the same operation as `mul`.
    assert!(rt_close(a * b, ab, 0.0));
}

#[test]
fn rigid_mul_v4_and_rotate_transform_agree() {
    // math.mul(RigidTransform, float4) @ 0x92cd920 rotates xyz, scales the translation by w and
    // passes w straight through.
    let a = RigidTransform::new(quaternion::euler_xyz(float3::new(0.4, 0.5, 0.6)), float3::new(1.0, -2.0, 3.0));
    let p = float4::new(2.0, 3.0, -4.0, 1.0);
    let got = RigidTransform::mul_v4(a, p);
    assert!(v3_close(float3::new(got.x, got.y, got.z), RigidTransform::transform(a, float3::new(2.0, 3.0, -4.0)), 1e-5));
    assert_eq!(got.w, 1.0);
    // With w = 0 the translation drops out entirely and the result is all zeros.
    assert_eq!(
        RigidTransform::mul_v4(a, float4::new(0.0, 0.0, 0.0, 0.0)),
        float4::new(0.0, 0.0, 0.0, 0.0)
    );
    // With w = 1 and a zero position the scaling term is what remains.
    let scaled = RigidTransform::mul_v4(a, float4::new(0.0, 0.0, 0.0, 2.0));
    assert!(v3_close(float3::new(scaled.x, scaled.y, scaled.z), a.pos * 2.0, 0.0));

    // math.rotate @ 0x92cdac0 ignores the translation; math.transform @ 0x92cdc10 adds it.
    let d = float3::new(1.5, -2.5, 3.5);
    assert!(v3_close(RigidTransform::rotate(a, d), quaternion::mul_qv(a.rot, d), 1e-5));
    assert!(v3_close(RigidTransform::transform(a, d), RigidTransform::rotate(a, d) + a.pos, 0.0));
}

#[test]
fn rigid_to_float4x4_round_trips() {
    // float4x4(RigidTransform) @ 0x934fa60 embeds the rotation columns with 0.0 below each and
    // puts the translation in the fourth column; math.RigidTransform(float4x4) @ 0x92cd3d0 reads
    // it back out.
    let t = RigidTransform::new(quaternion::euler_yzx(float3::new(0.3, 1.1, -0.4)), float3::new(12.0, -3.5, 7.25));
    let m = RigidTransform::to_float4x4(t);
    assert_eq!(m.c3, float4::new(12.0, -3.5, 7.25, 1.0));
    let r = float3x3::from_quaternion(t.rot);
    assert!(v3_close(float3::new(m.c0.x, m.c0.y, m.c0.z), r.c0, 0.0));
    assert!(v3_close(float3::new(m.c1.x, m.c1.y, m.c1.z), r.c1, 0.0));
    assert!(v3_close(float3::new(m.c2.x, m.c2.y, m.c2.z), r.c2, 0.0));

    let back = RigidTransform::from_matrix4(m);
    assert!(rt_close(back, t, 1e-5), "{back:?} vs {t:?}");

    // The `math` wrappers are the same three constructions.
    assert_eq!(rigid_transform(t.rot, t.pos), t);
    assert!(rt_close(rigid_transform_m3(r, t.pos), t, 1e-5));
    assert!(rt_close(rigid_transform_m4(m), t, 1e-5));

    // And the matrix moves a point the way `transform` does.
    let p = float4::new(1.0, 2.0, 3.0, 1.0);
    let moved = m * p;
    let via_transform = RigidTransform::transform(t, float3::new(1.0, 2.0, 3.0));
    assert!(
        v3_close(
            float3::new(moved.x, moved.y, moved.z),
            via_transform,
            1e-4
        ),
        "{} vs {}",
        float3::new(moved.x, moved.y, moved.z),
        via_transform
    );
}

#[test]
fn fastinverse_is_the_rigid_inverse() {
    // math.fastinverse(float4x4) @ 0x929eac0: the transposed rotation with 0.0 below each column,
    // and the translation negated through the same matrix.
    let t = RigidTransform::new(quaternion::euler_zxy(float3::new(0.3, -0.9, 1.2)), float3::new(4.0, -5.5, 6.0));
    let m = RigidTransform::to_float4x4(t);
    let f = fastinverse(m);
    assert_eq!(f.c0.w, 0.0);
    assert_eq!(f.c1.w, 0.0);
    assert_eq!(f.c2.w, 0.0);
    assert_eq!(f.c3.w, 1.0);
    // The rotation part is the transpose of the input's.
    assert_eq!(f.c0.x, m.c0.x);
    assert_eq!(f.c0.y, m.c1.x);
    assert_eq!(f.c0.z, m.c2.x);
    assert_eq!(f.c1.x, m.c0.y);
    assert_eq!(f.c1.y, m.c1.y);
    assert_eq!(f.c1.z, m.c2.y);
    assert_eq!(f.c2.x, m.c0.z);
    assert_eq!(f.c2.y, m.c1.z);
    assert_eq!(f.c2.z, m.c2.z);
    // It really is an inverse: the general 4×4 inverse agrees, and the product is the identity.
    assert!(
        m4_close(f, m.inverse(), 1e-4),
        "fastinverse disagrees with the general inverse: {f:?} vs {:?}",
        m.inverse()
    );
    assert!(m4_close(m * f, float4x4::identity, 1e-4));
    assert!(m4_close(f * m, float4x4::identity, 1e-4));
    // fastinverse is exact on the transpose and the sign flip, so a pure rotation round trips
    // through it with no rounding at all.
    let rot_only = float4x4::rotate(t.rot);
    assert_eq!(fastinverse(fastinverse(rot_only)), rot_only);
}

#[test]
fn hash_rigid_and_hashwide_match_the_bodies_mixes() {
    // math.hash(RigidTransform) @ 0x92cdd90 and GetHashCode @ 0x938e920 are the same seven
    // multiplies; math.hashwide(RigidTransform) @ 0x92cde30 is four lanes with their own primes.
    // The expected values below were computed from the body's constants independently.
    let cases = [
        RigidTransform::identity,
        RigidTransform::new(
            quaternion::new(0.5, 0.5, 0.5, 0.5),
            float3::new(1.0, 2.0, 3.0),
        ),
        RigidTransform::new(
            quaternion::new(-0.25, 1.5, 0.0, 0.75),
            float3::new(1e-30, 0.0, -2.5),
        ),
    ];
    let expect = [0x49ff_b9d4u32, 0x6f3f_b9d4, 0x137e_7174];
    let expect_wide = [
        [0x189a_d918u32, 0x189a_d918, 0x189a_d918, 0x691a_d918],
        [0xda1a_d918, 0x239a_d918, 0x611a_d918, 0xc4da_d918],
        [0x4f00_1cb8, 0x7b5a_d918, 0xb67a_43f8, 0xb6fa_d918],
    ];
    for ((t, want), wide) in cases.iter().zip(expect.iter()).zip(expect_wide.iter()) {
        assert_eq!(hash_rigid(*t), *want, "{t:?}");
        let h = hashwide_rigid(*t);
        assert_eq!([h.x, h.y, h.z, h.w], *wide, "{t:?}");
    }
    // `Hash` writes the same value `math.hash` returns, so a HashMap keyed by RigidTransform
    // buckets the way the original's GetHashCode would.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    for t in cases {
        let mut a = DefaultHasher::new();
        t.hash(&mut a);
        let mut b = DefaultHasher::new();
        hash_rigid(t).hash(&mut b);
        assert_eq!(a.finish(), b.finish());
    }
}

#[test]
fn display_is_the_recovered_format_string() {
    // RigidTransform.ToString() @ 0x938e9c0 formats with the literal at VA 0x18dab29d8:
    // RigidTransform(({0}f, {1}f, {2}f, {3}f),  ({4}f, {5}f, {6}f))
    let t = RigidTransform::new(
        quaternion::new(1.5, -2.5, 0.0, 1.0),
        float3::new(4.0, -5.5, 6.0),
    );
    assert_eq!(
        t.to_string(),
        "RigidTransform((1.5f, -2.5f, 0f, 1f),  (4f, -5.5f, 6f))"
    );
    assert_eq!(
        RigidTransform::identity.to_string(),
        "RigidTransform((0f, 0f, 0f, 1f),  (0f, 0f, 0f))"
    );
}

#[test]
fn equality_is_the_bodies_seven_component_compare() {
    // Equals @ 0x938e780 compares rot.xyzw then pos.xyz, component by component. Rust's derived
    // PartialEq walks the fields in declaration order, which is the same seven comparisons.
    let a = RigidTransform::new(quaternion::new(1.0, 2.0, 3.0, 4.0), float3::new(5.0, 6.0, 7.0));
    let mut b = a;
    assert_eq!(a, b);
    b.pos.z = 7.5;
    assert_ne!(a, b);
    b = a;
    b.rot.value.w = 4.5;
    assert_ne!(a, b);
    // +0.0 and -0.0 compare equal, as they do in the original's `==` on floats.
    assert_eq!(
        RigidTransform::new(quaternion::new(0.0, 0.0, 0.0, 1.0), float3::new(0.0, 0.0, 0.0)),
        RigidTransform::new(quaternion::new(-0.0, -0.0, -0.0, 1.0), float3::new(-0.0, -0.0, -0.0))
    );
    // Default is the zeroed struct, not the identity: the original has no parameterless
    // constructor that would fix that up.
    assert_eq!(RigidTransform::default().rot, quaternion::default());
    assert_eq!(RigidTransform::default().pos, float3::zero);
}
