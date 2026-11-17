//! Pins the Euler-angle families transcribed in `src/euler.rs`.
//!
//! Two independent sources have to agree here, which is what makes these tests worth having: the
//! shipped bodies (transcribed term for term) and the rotation algebra the original's own doc
//! comments state — `EulerXYZ` *is* `mul(rotateZ, mul(rotateY, rotateX))`. If a sign in the table
//! were wrong, the composition check would fail; if a body were misread, the
//! `float3x3`-against-`quaternion` check would fail, because those two were transcribed from
//! different functions.

use rrising_math::{float3, float3x3, float4, float4x4, quaternion, RotationOrder};
fn approx(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() <= eps
}

fn q_close(a: quaternion, b: quaternion, eps: f32) -> bool {
    approx(a.value.x, b.value.x, eps)
        && approx(a.value.y, b.value.y, eps)
        && approx(a.value.z, b.value.z, eps)
        && approx(a.value.w, b.value.w, eps)
}

fn m3_close(a: float3x3, b: float3x3, eps: f32) -> bool {
    for (x, y) in [
        (a.c0.x, b.c0.x),
        (a.c0.y, b.c0.y),
        (a.c0.z, b.c0.z),
        (a.c1.x, b.c1.x),
        (a.c1.y, b.c1.y),
        (a.c1.z, b.c1.z),
        (a.c2.x, b.c2.x),
        (a.c2.y, b.c2.y),
        (a.c2.z, b.c2.z),
    ] {
        if !approx(x, y, eps) {
            return false;
        }
    }
    true
}

const ANGLES: [(f32, f32, f32); 4] = [
    (0.4, -0.9, 1.2),
    (0.0, 0.0, 0.0),
    (1.5707964, -1.5707964, 0.3),
    (-0.7, 2.2, -1.1),
];

#[test]
fn each_quaternion_order_is_the_documented_product_of_axis_rotations() {
    // The original's doc comments state these identities; the bodies implement them.
    for (x, y, z) in ANGLES {
        let v = float3::new(x, y, z);
        let (rx, ry, rz) = (
            quaternion::rotate_x(x),
            quaternion::rotate_y(y),
            quaternion::rotate_z(z),
        );
        assert!(
            q_close(quaternion::euler_xyz(v), rz * (ry * rx), 1e-5),
            "XYZ at {v}: {} vs {}",
            quaternion::euler_xyz(v),
            rz * (ry * rx)
        );
        assert!(
            q_close(quaternion::euler_xzy(v), ry * (rz * rx), 1e-5),
            "XZY at {v}: {} vs {}",
            quaternion::euler_xzy(v),
            ry * (rz * rx)
        );
        assert!(
            q_close(quaternion::euler_yxz(v), rz * (rx * ry), 1e-5),
            "YXZ at {v}: {} vs {}",
            quaternion::euler_yxz(v),
            rz * (rx * ry)
        );
        assert!(
            q_close(quaternion::euler_yzx(v), rx * (rz * ry), 1e-5),
            "YZX at {v}: {} vs {}",
            quaternion::euler_yzx(v),
            rx * (rz * ry)
        );
        assert!(
            q_close(quaternion::euler_zxy(v), ry * (rx * rz), 1e-5),
            "ZXY at {v}: {} vs {}",
            quaternion::euler_zxy(v),
            ry * (rx * rz)
        );
        assert!(
            q_close(quaternion::euler_zyx(v), rx * (ry * rz), 1e-5),
            "ZYX at {v}: {} vs {}",
            quaternion::euler_zyx(v),
            rx * (ry * rz)
        );
    }
}

#[test]
fn the_scalar_overloads_match_the_vector_ones() {
    for (x, y, z) in ANGLES {
        let v = float3::new(x, y, z);
        assert!(q_close(
            quaternion::euler_xyz_3(x, y, z),
            quaternion::euler_xyz(v),
            0.0
        ));
        assert!(q_close(
            quaternion::euler_xzy_3(x, y, z),
            quaternion::euler_xzy(v),
            0.0
        ));
        assert!(q_close(
            quaternion::euler_yxz_3(x, y, z),
            quaternion::euler_yxz(v),
            0.0
        ));
        assert!(q_close(
            quaternion::euler_yzx_3(x, y, z),
            quaternion::euler_yzx(v),
            0.0
        ));
        assert!(q_close(
            quaternion::euler_zxy_3(x, y, z),
            quaternion::euler_zxy(v),
            0.0
        ));
        assert!(q_close(
            quaternion::euler_zyx_3(x, y, z),
            quaternion::euler_zyx(v),
            0.0
        ));
    }
}

#[test]
fn the_matrix_eulers_are_the_same_rotation_as_the_quaternion_ones() {
    // Transcribed from different bodies (float3x3.Euler* and quaternion.Euler*), so agreement is
    // evidence about both.
    for (x, y, z) in ANGLES {
        let v = float3::new(x, y, z);
        for (m, q, name) in [
            (float3x3::euler_xyz(v), quaternion::euler_xyz(v), "XYZ"),
            (float3x3::euler_xzy(v), quaternion::euler_xzy(v), "XZY"),
            (float3x3::euler_yxz(v), quaternion::euler_yxz(v), "YXZ"),
            (float3x3::euler_yzx(v), quaternion::euler_yzx(v), "YZX"),
            (float3x3::euler_zxy(v), quaternion::euler_zxy(v), "ZXY"),
            (float3x3::euler_zyx(v), quaternion::euler_zyx(v), "ZYX"),
        ] {
            assert!(
                m3_close(m, float3x3::from_quaternion(q), 1e-5),
                "{name} at {v}: matrix {m:?} is not the quaternion's basis {q:?}",
            );
        }
    }
}

#[test]
fn the_4x4_eulers_embed_the_3x3_ones() {
    for (x, y, z) in ANGLES {
        let v = float3::new(x, y, z);
        let m = float4x4::euler_zyx(v);
        let b = float3x3::euler_zyx(v);
        assert!(
            m3_close(
                float3x3::new(b.c0, b.c1, b.c2),
                float3x3::new(
                    float3::new(m.c0.x, m.c0.y, m.c0.z),
                    float3::new(m.c1.x, m.c1.y, m.c1.z),
                    float3::new(m.c2.x, m.c2.y, m.c2.z),
                ),
                0.0
            ),
            "{m:?}"
        );
        // The body stores 0.0 in the fourth element of each of the first three columns and 1.0 in
        // the last.
        assert_eq!((m.c0.w, m.c1.w, m.c2.w, m.c3.w), (0.0, 0.0, 0.0, 1.0));
        assert_eq!((m.c3.x, m.c3.y, m.c3.z), (0.0, 0.0, 0.0));
        assert!(m3_close(
            float3x3::euler_xyz_3(x, y, z),
            float3x3::euler_xyz(v),
            0.0
        ));
    }
}

#[test]
fn ortho_off_center_maps_the_view_box_onto_the_clip_cube() {
    // The body's own terms: 2/(r-l), 2/(t-b), -2/(f-n) and the negated midpoints. Depth is fed in
    // eye space, where the camera looks down -z, so the near plane sits at z = -near; that is the
    // input the original's `Ortho` pair is built to map onto -1 and +1.
    let m = float4x4::ortho_off_center(-2.0, 4.0, -1.0, 3.0, 0.5, 12.0);
    for (x, y, z, want_x, want_y, want_z) in [
        (-2.0, -1.0, -0.5, -1.0, -1.0, -1.0),
        (4.0, 3.0, -12.0, 1.0, 1.0, 1.0),
        (1.0, 1.0, -6.25, 0.0, 0.0, 0.0),
    ] {
        let p = m * float4::new(x, y, z, 1.0);
        assert!(approx(p.x, want_x, 1e-5), "x {x} -> {}", p.x);
        assert!(approx(p.y, want_y, 1e-5), "y {y} -> {}", p.y);
        assert!(approx(p.z, want_z, 1e-5), "z {z} -> {}", p.z);
        assert_eq!(p.w, 1.0);
    }
}

#[test]
fn the_shipped_rotation_order_enum_starts_at_zero_and_defaults_to_zxy() {
    // dump.cs: `XYZ = 0, XZY = 1, YXZ = 2, YZX = 3, ZXY = 4, ZYX = 5, Default = 4`, and
    // `Euler(float3 xyz, math.RotationOrder order = 4)`.
    assert_eq!(RotationOrder::XYZ as i32, 0);
    assert_eq!(RotationOrder::ZYX as i32, 5);
    assert_eq!(
        rrising_math::math::DEFAULT_ROTATION_ORDER,
        RotationOrder::ZXY
    );
    let v = float3::new(0.4, -0.9, 1.2);
    assert!(q_close(quaternion::euler(v), quaternion::euler_zxy(v), 0.0));
    assert!(q_close(
        quaternion::euler_order(v, RotationOrder::YXZ),
        quaternion::euler_yxz(v),
        0.0
    ));
}
