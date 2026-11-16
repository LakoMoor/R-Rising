//! Parity tests for the ported math.
//!
//! The reference values come from the original's own definitions (half-angle construction,
//! column-major composition, the `mul` two-cross form, Unity's [0,1] NDC depth), not from the port's
//! behaviour, so a change that matches the port but not the original fails here.

use rrising_math::*;

fn approx(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

fn v3_close(a: float3, b: float3, tol: f32) -> bool {
    approx(a.x, b.x, tol) && approx(a.y, b.y, tol) && approx(a.z, b.z, tol)
}

fn q_close(a: quaternion, b: quaternion, tol: f32) -> bool {
    approx(a.value.x, b.value.x, tol)
        && approx(a.value.y, b.value.y, tol)
        && approx(a.value.z, b.value.z, tol)
        && approx(a.value.w, b.value.w, tol)
}

#[test]
fn vector_basics() {
    assert_eq!(
        float3::new(1.0, 2.0, 3.0).dot(float3::new(4.0, 5.0, 6.0)),
        32.0
    );
    assert!(v3_close(
        float3::new(1.0, 0.0, 0.0).cross(float3::new(0.0, 1.0, 0.0)),
        float3::new(0.0, 0.0, 1.0),
        1e-7
    ));
    assert!(v3_close(
        normalize(float3::new(0.0, 3.0, 4.0)),
        float3::new(0.0, 0.6, 0.8),
        1e-6
    ));
    assert_eq!(lengthsq(float3::new(1.0, 2.0, 3.0)), 14.0);
    assert!(approx(
        distance(float3::zero, float3::new(3.0, 0.0, 4.0)),
        5.0,
        1e-6
    ));
    assert!(approx(lerp(0.0, 10.0, 0.5), 5.0, 1e-7));
    assert!(approx(clamp(5.0, 0.0, 3.0), 3.0, 0.0));
    assert!(approx(saturate(2.0), 1.0, 0.0));
}

#[test]
fn swizzles_match_declaration_order() {
    let v = float3::new(1.0, 2.0, 3.0);
    assert!(v3_close(v.xzy(), float3::new(1.0, 3.0, 2.0), 0.0));
    assert!(v3_close(v.zyx(), float3::new(3.0, 2.0, 1.0), 0.0));
    assert!(v3_close(v.yzx(), float3::new(2.0, 3.0, 1.0), 0.0));
    let w = float4::new(1.0, 2.0, 3.0, 4.0);
    assert!(v3_close(w.xyz(), float3::new(1.0, 2.0, 3.0), 0.0));
    assert!(approx(w.dot(float4::new(1.0, 1.0, 1.0, 1.0)), 10.0, 0.0));
}

#[test]
fn quaternion_half_angle_construction() {
    // RotateX(90deg) is (sin45, 0, 0, cos45) — the original's half-angle form.
    let q = quaternion::rotate_x(std::f32::consts::FRAC_PI_2);
    assert!(q_close(
        q,
        quaternion::new(
        core::f32::consts::FRAC_1_SQRT_2,
        0.0,
        0.0,
        core::f32::consts::FRAC_1_SQRT_2,
    ),
        1e-6
    ));
}

#[test]
fn quaternion_rotates_basis_vectors() {
    let q = quaternion::rotate_z(std::f32::consts::FRAC_PI_2);
    assert!(v3_close(
        quaternion::transform(q, float3::new(1.0, 0.0, 0.0)),
        float3::new(0.0, 1.0, 0.0),
        1e-6
    ));
    assert!(v3_close(
        quaternion::rotate_normal_x(q),
        float3::new(0.0, 1.0, 0.0),
        1e-6
    ));
    let p = float3::new(1.5, -2.0, 3.5);
    let inv = quaternion::inverse(q);
    assert!(v3_close(
        quaternion::transform(inv, quaternion::transform(q, p)),
        p,
        1e-5
    ));
}

#[test]
fn quaternion_multiply_is_not_commutative() {
    let a = quaternion::rotate_x(0.3);
    let b = quaternion::rotate_y(0.7);
    let ab = a * b;
    let ba = b * a;
    assert!(
        !q_close(ab, ba, 1e-4),
        "q1*q2 must not equal q2*q1 for distinct axes"
    );
    assert!(q_close(ab * quaternion::identity, ab, 1e-7));
    assert!(q_close(quaternion::identity * ab, ab, 1e-7));
}

#[test]
fn matrices_are_column_major() {
    // IL2CPP puts c0 at 0x0 and c1 at 0x10: translation lives in c3, not in the last row.
    let t = float4x4::translate(float3::new(7.0, 8.0, 9.0));
    assert!(approx(t.c3.x, 7.0, 0.0) && approx(t.c3.y, 8.0, 0.0) && approx(t.c3.z, 9.0, 0.0));
    assert!(approx(t.c3.w, 1.0, 0.0));
    assert!(v3_close(t.position(), float3::new(7.0, 8.0, 9.0), 0.0));
    let p = t * float4::new(1.0, 2.0, 3.0, 1.0);
    assert!(v3_close(
        float3::new(p.x, p.y, p.z),
        float3::new(8.0, 10.0, 12.0),
        1e-6
    ));
}

#[test]
fn trs_composes_in_the_original_order() {
    let pos = float3::new(1.0, 2.0, 3.0);
    let rot = quaternion::rotate_z(std::f32::consts::FRAC_PI_2);
    let scl = float3::new(2.0, 2.0, 2.0);
    let m = float4x4::trs(pos, rot, scl);
    let x_axis = m * float4::new(1.0, 0.0, 0.0, 0.0);
    assert!(v3_close(
        float3::new(x_axis.x, x_axis.y, x_axis.z),
        float3::new(0.0, 2.0, 0.0),
        1e-5
    ));
    assert!(v3_close(m.position(), pos, 1e-6));
}

#[test]
fn matrix_inverse_round_trips() {
    let m = float4x4::trs(
        float3::new(1.0, -2.0, 3.0),
        quaternion::euler_zyx_3(0.4, -0.9, 1.2),
        float3::new(2.0, 3.0, 4.0),
    );
    let id = m * m.inverse();
    for i in 0..4 {
        let r = id.row(i);
        let got = [r.x, r.y, r.z, r.w];
        for j in 0..4 {
            let want = if i == j { 1.0 } else { 0.0 };
            assert!(
                approx(got[j], want, 1e-3),
                "row {i} col {j}: {} vs {want}",
                got[j]
            );
        }
    }
    // A singular matrix inverts to zero, as the original's guard does.
    let flat = float4x4::scale(float3::new(1.0, 0.0, 1.0));
    assert!(approx(flat.determinant(), 0.0, 1e-7));
    assert!(approx(flat.inverse().c0.x, 0.0, 0.0));
}

#[test]
fn determinant_matches_known_values() {
    assert!(approx(float4x4::identity.determinant(), 1.0, 1e-6));
    let s = float4x4::scale(float3::new(2.0, 3.0, 4.0));
    assert!(approx(s.determinant(), 24.0, 1e-5));
    assert!(approx(float3x3::identity.determinant(), 1.0, 1e-6));
    let r = float3x3::from_quaternion(quaternion::rotate_x(0.9));
    assert!(
        approx(r.determinant(), 1.0, 1e-5),
        "a rotation preserves volume"
    );
}

#[test]
fn quaternion_to_matrix_agrees_with_direct_rotation() {
    let q = quaternion::euler_zyx_3(0.3, -0.7, 1.1);
    let m = float4x4::rotate(q);
    let v = float3::new(1.0, 2.0, 3.0);
    let by_matrix = m * float4::new(v.x, v.y, v.z, 0.0);
    let by_quat = quaternion::transform(q, v);
    assert!(
        v3_close(
            float3::new(by_matrix.x, by_matrix.y, by_matrix.z),
            by_quat,
            1e-5
        ),
        "matrix and quaternion rotation must agree: {by_matrix:?} vs {by_quat:?}"
    );
}

#[test]
fn projection_maps_planes_to_the_ends_of_ndc_depth() {
    // Unity's NDC depth is [0,1]; near and far must land on the two ends, in either order.
    let p = float4x4::perspective_fov(1.0, 16.0 / 9.0, 0.1, 100.0);
    let near = p * float4::new(0.0, 0.0, -0.1, 1.0);
    let far = p * float4::new(0.0, 0.0, -100.0, 1.0);
    let ndc_near = near.z / near.w;
    let ndc_far = far.z / far.w;
    // `PerspectiveFov` is the [-1,1] clip form: near maps to -1 and far to +1.
    assert!(approx(ndc_near, -1.0, 1e-3), "near mapped to {ndc_near}");
    assert!(approx(ndc_far, 1.0, 1e-3), "far mapped to {ndc_far}");
}

#[test]
fn scalar_helpers() {
    assert!(approx(radians(180.0), std::f32::consts::PI, 1e-5));
    assert!(approx(degrees(std::f32::consts::PI), 180.0, 1e-4));
    assert!(approx(smoothstep(0.0, 1.0, 0.5), 0.5, 1e-6));
    assert_eq!(step(1.0, 0.5), 0.0);
    assert_eq!(step(1.0, 1.5), 1.0);
    assert_eq!(sign(-3.0), -1.0);
    assert_eq!(abs(-2.5), 2.5);
    assert!(approx(rsqrt(4.0), 0.5, 1e-7));
    // The dump has `math.ispow2` / `math.ceilpow2`; there is no `nextpoweroftwo` in this build.
    assert_eq!(ceilpow2(5), 8);
    assert!(ispow2(64));
    assert!(!ispow2(63));
}

#[test]
fn select_and_masks() {
    let a = float3::new(1.0, 2.0, 3.0);
    let b = float3::new(9.0, 8.0, 7.0);
    let m = less3(a, b);
    assert!(m.x && m.y && m.z);
    assert!(v3_close(select3(m, a, b), a, 0.0));
    assert!(v3_close(min3(a, b), a, 0.0));
    assert!(v3_close(max3(a, b), b, 0.0));
    assert!(v3_close(
        clamp3(b, a, float3::splat(2.5)),
        float3::splat(2.5),
        0.0
    ));
}
