//! The Euler-angle constructors of `quaternion`, `float3x3` and `float4x4`, and the off-center
//! orthographic projection.
//!
//! Bodies, all from `evidence/decompiled/Unity.Mathematics.dll.c`:
//!
//! * `quaternion.EulerXYZ_8 @ 0x5b90b90`, `EulerXZY_9 @ 0x5b90d00`, `EulerYXZ_10 @ 0x5b90e70`,
//!   `EulerYZX_11 @ 0x5b90fe0`, `EulerZXY_12 @ 0x5b90a20`, `EulerZYX_13 @ 0x5b91150`; the
//!   `(float, float, float)` overloads @ 0x9375dc0-0x9375f00 pack the three scalars into a `float3`
//!   and call these.
//! * `float3x3.EulerXYZ_66 @ 0x9338c40` … `EulerZYX_71 @ 0x9339490`.
//! * `float4x4.EulerXYZ_101 @ 0x934fe70` … `EulerZYX_106 @ 0x9350860` — the same nine products in
//!   the same grouping, with `0.0` in the fourth element of each column and `1.0` in the last.
//! * `float4x4.OrthoOffCenter_112 @ 0x9351790`.
//!
//! Every one of them halves the angles with the `0.5f` at 0x18cf28a88 and flips signs either by
//! multiplying by the `-1.0f` at 0x18cf29990 (the quaternions) or by xoring the sign bit with the
//! mask at 0x18cf2aee0 (the matrices). The multiplication order below is the body's, term for term.

use crate::matrix::{float3x3, float4x4};
use crate::quat::quaternion;
use crate::vector::{float3, float4};
use crate::RotationOrder;

/// The six quaternion orders differ only in the sign vector applied to the odd term. Read off the
/// `* -1.0f` in each body's stores, and equal to the shipped Unity.Mathematics source's
/// `float4(-1,1,-1,1)`-style literals.
fn quaternion_euler(xyz: float3, signs: float4) -> quaternion {
    let h = xyz * 0.5;
    let (sx, cx) = h.x.sin_cos();
    let (sy, cy) = h.y.sin_cos();
    let (sz, cz) = h.z.sin_cos();
    let s = float3::new(sx, sy, sz);
    let c = float3::new(cx, cy, cz);
    // `float4(s.xyz, c.x) * c.yxxy * c.zzyz + s.yxxy * s.zzyz * float4(c.xyz, s.x) * signs`
    let even = float4::new(s.x, s.y, s.z, c.x) * c.yxxy() * c.zzyz();
    let odd = s.yxxy() * s.zzyz() * float4::new(c.x, c.y, c.z, s.x) * signs;
    quaternion::from_float4(even + odd)
}

impl quaternion {
    /// `quaternion.EulerXYZ(float3 xyz)` — rotate about x, then y, then z.
    #[inline]
    pub fn euler_xyz(xyz: float3) -> Self {
        quaternion_euler(xyz, float4::new(-1.0, 1.0, -1.0, 1.0))
    }

    /// `quaternion.EulerXZY(float3 xyz)` — x, then z, then y.
    #[inline]
    pub fn euler_xzy(xyz: float3) -> Self {
        quaternion_euler(xyz, float4::new(1.0, 1.0, -1.0, -1.0))
    }

    /// `quaternion.EulerYXZ(float3 xyz)` — y, then x, then z.
    #[inline]
    pub fn euler_yxz(xyz: float3) -> Self {
        quaternion_euler(xyz, float4::new(-1.0, 1.0, 1.0, -1.0))
    }

    /// `quaternion.EulerYZX(float3 xyz)` — y, then z, then x.
    #[inline]
    pub fn euler_yzx(xyz: float3) -> Self {
        quaternion_euler(xyz, float4::new(-1.0, -1.0, 1.0, 1.0))
    }

    /// `quaternion.EulerZXY(float3 xyz)` — z, then x, then y; the original's default order.
    #[inline]
    pub fn euler_zxy(xyz: float3) -> Self {
        quaternion_euler(xyz, float4::new(1.0, -1.0, -1.0, 1.0))
    }

    /// `quaternion.EulerZYX(float3 xyz)` — z, then y, then x.
    #[inline]
    pub fn euler_zyx(xyz: float3) -> Self {
        quaternion_euler(xyz, float4::new(1.0, -1.0, 1.0, -1.0))
    }

    /// `quaternion.EulerXYZ(float x, float y, float z)` @ 0x9375dc0 and the five siblings
    /// @ 0x9375e00-0x9375f00: the dump's scalar overloads, named `_3` because Rust has no
    /// overloading.
    #[inline]
    pub fn euler_xyz_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_xyz(float3::new(x, y, z))
    }

    /// `quaternion.EulerXZY(float, float, float)` @ 0x9375e00.
    #[inline]
    pub fn euler_xzy_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_xzy(float3::new(x, y, z))
    }

    /// `quaternion.EulerYXZ(float, float, float)` @ 0x9375e40.
    #[inline]
    pub fn euler_yxz_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_yxz(float3::new(x, y, z))
    }

    /// `quaternion.EulerYZX(float, float, float)` @ 0x9375e80.
    #[inline]
    pub fn euler_yzx_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_yzx(float3::new(x, y, z))
    }

    /// `quaternion.EulerZXY(float, float, float)` @ 0x9375ec0.
    #[inline]
    pub fn euler_zxy_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_zxy(float3::new(x, y, z))
    }

    /// `quaternion.EulerZYX(float, float, float)` @ 0x9375f00.
    #[inline]
    pub fn euler_zyx_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_zyx(float3::new(x, y, z))
    }

    /// `quaternion.Euler(float3 xyz, math.RotationOrder order = RotationOrder.ZXY)` — the default
    /// is what the dump records: `Euler(float3 xyz, math.RotationOrder order = 4)`, and the shipped
    /// enum's 4 is `ZXY`.
    #[inline]
    pub fn euler_order(xyz: float3, order: RotationOrder) -> Self {
        match order {
            RotationOrder::XYZ => Self::euler_xyz(xyz),
            RotationOrder::XZY => Self::euler_xzy(xyz),
            RotationOrder::YXZ => Self::euler_yxz(xyz),
            RotationOrder::YZX => Self::euler_yzx(xyz),
            RotationOrder::ZXY => Self::euler_zxy(xyz),
            RotationOrder::ZYX => Self::euler_zyx(xyz),
        }
    }

    /// `quaternion.Euler(float3 xyz, RotationOrder order = ZXY)`.
    #[inline]
    pub fn euler(xyz: float3) -> Self {
        Self::euler_zxy(xyz)
    }

    /// `quaternion.Euler(float x, float y, float z, RotationOrder order = ZXY)`.
    #[inline]
    pub fn euler_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_zxy(float3::new(x, y, z))
    }
}

impl float3x3 {
    /// `float3x3.EulerXYZ(float3 xyz)` @ 0x9338c40 — the body's nine products, column by column.
    #[inline]
    pub fn euler_xyz(xyz: float3) -> Self {
        let (sx, cx) = xyz.x.sin_cos();
        let (sy, cy) = xyz.y.sin_cos();
        let (sz, cz) = xyz.z.sin_cos();
        Self::new(
            float3::new(cz * cy, sz * cy, -(sy)),
            float3::new(cz * sx * sy - sz * cx, sx * sy * sz + cx * cz, cy * sx),
            float3::new(cx * cz * sy + sz * sx, cx * sy * sz - cz * sx, cx * cy),
        )
    }

    /// `float3x3.EulerXZY(float3 xyz)` @ 0x9338e00.
    #[inline]
    pub fn euler_xzy(xyz: float3) -> Self {
        let (sx, cx) = xyz.x.sin_cos();
        let (sy, cy) = xyz.y.sin_cos();
        let (sz, cz) = xyz.z.sin_cos();
        Self::new(
            float3::new(cz * cy, sz, -(cz) * sy),
            float3::new(sx * sy - cx * cy * sz, cx * cz, cx * sy * sz + cy * sx),
            float3::new(cy * sx * sz + cx * sy, -(cz) * sx, cx * cy - sx * sy * sz),
        )
    }

    /// `float3x3.EulerYXZ(float3 xyz)` @ 0x9338fb0.
    #[inline]
    pub fn euler_yxz(xyz: float3) -> Self {
        let (sx, cx) = xyz.x.sin_cos();
        let (sy, cy) = xyz.y.sin_cos();
        let (sz, cz) = xyz.z.sin_cos();
        Self::new(
            float3::new(cz * cy - sx * sy * sz, cz * sx * sy + sz * cy, -(cx) * sy),
            float3::new(-(cx) * sz, cx * cz, sx),
            float3::new(cy * sx * sz + cz * sy, sz * sy - cz * cy * sx, cx * cy),
        )
    }

    /// `float3x3.EulerYZX(float3 xyz)` @ 0x9339150.
    #[inline]
    pub fn euler_yzx(xyz: float3) -> Self {
        let (sx, cx) = xyz.x.sin_cos();
        let (sy, cy) = xyz.y.sin_cos();
        let (sz, cz) = xyz.z.sin_cos();
        Self::new(
            float3::new(cz * cy, cx * cy * sz + sx * sy, cy * sx * sz - cx * sy),
            float3::new(-(sz), cx * cz, cz * sx),
            float3::new(cz * sy, cx * sy * sz - cy * sx, sx * sy * sz + cx * cy),
        )
    }

    /// `float3x3.EulerZXY(float3 xyz)` @ 0x9339300.
    #[inline]
    pub fn euler_zxy(xyz: float3) -> Self {
        let (sx, cx) = xyz.x.sin_cos();
        let (sy, cy) = xyz.y.sin_cos();
        let (sz, cz) = xyz.z.sin_cos();
        Self::new(
            float3::new(sx * sy * sz + cz * cy, sz * cx, cy * sx * sz - cz * sy),
            float3::new(cz * sx * sy - sz * cy, cx * cz, cz * cy * sx + sz * sy),
            float3::new(cx * sy, -(sx), cx * cy),
        )
    }

    /// `float3x3.EulerZYX(float3 xyz)` @ 0x9339490.
    #[inline]
    pub fn euler_zyx(xyz: float3) -> Self {
        let (sx, cx) = xyz.x.sin_cos();
        let (sy, cy) = xyz.y.sin_cos();
        let (sz, cz) = xyz.z.sin_cos();
        Self::new(
            float3::new(cz * cy, cz * sx * sy + sz * cx, sz * sx - cx * cz * sy),
            float3::new(-(cy) * sz, cx * cz - sx * sy * sz, cx * sy * sz + cz * sx),
            float3::new(sy, -(cy) * sx, cx * cy),
        )
    }

    /// `float3x3.EulerXYZ(float x, float y, float z)` @ 0x9339650 and the five siblings
    /// @ 0x93396a0-0x93397e0.
    #[inline]
    pub fn euler_xyz_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_xyz(float3::new(x, y, z))
    }

    /// `float3x3.EulerXZY(float, float, float)` @ 0x93396a0.
    #[inline]
    pub fn euler_xzy_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_xzy(float3::new(x, y, z))
    }

    /// `float3x3.EulerYXZ(float, float, float)` @ 0x93396f0.
    #[inline]
    pub fn euler_yxz_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_yxz(float3::new(x, y, z))
    }

    /// `float3x3.EulerYZX(float, float, float)` @ 0x9339740.
    #[inline]
    pub fn euler_yzx_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_yzx(float3::new(x, y, z))
    }

    /// `float3x3.EulerZXY(float, float, float)` @ 0x9339790.
    #[inline]
    pub fn euler_zxy_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_zxy(float3::new(x, y, z))
    }

    /// `float3x3.EulerZYX(float, float, float)` @ 0x93397e0.
    #[inline]
    pub fn euler_zyx_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_zyx(float3::new(x, y, z))
    }
}

impl float4x4 {
    /// `float4x4.EulerXYZ(float3 xyz)` @ 0x934fe70 — the `float3x3` product embedded in a 4x4: the
    /// body stores the same nine products, `0.0` in the fourth element of each of the first three
    /// columns and `1.0` in the last.
    #[inline]
    pub fn euler_xyz(xyz: float3) -> Self {
        embed3x3(float3x3::euler_xyz(xyz))
    }

    /// `float4x4.EulerXZY(float3 xyz)` @ 0x9350080.
    #[inline]
    pub fn euler_xzy(xyz: float3) -> Self {
        embed3x3(float3x3::euler_xzy(xyz))
    }

    /// `float4x4.EulerYXZ(float3 xyz)` @ 0x9350280.
    #[inline]
    pub fn euler_yxz(xyz: float3) -> Self {
        embed3x3(float3x3::euler_yxz(xyz))
    }

    /// `float4x4.EulerYZX(float3 xyz)` @ 0x9350470.
    #[inline]
    pub fn euler_yzx(xyz: float3) -> Self {
        embed3x3(float3x3::euler_yzx(xyz))
    }

    /// `float4x4.EulerZXY(float3 xyz)` @ 0x9350670.
    #[inline]
    pub fn euler_zxy(xyz: float3) -> Self {
        embed3x3(float3x3::euler_zxy(xyz))
    }

    /// `float4x4.EulerZYX(float3 xyz)` @ 0x9350860.
    #[inline]
    pub fn euler_zyx(xyz: float3) -> Self {
        embed3x3(float3x3::euler_zyx(xyz))
    }

    /// `float4x4.EulerXYZ(float, float, float)` @ 0x9350a70 and the five siblings
    /// @ 0x9350ac0-0x9350c00.
    #[inline]
    pub fn euler_xyz_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_xyz(float3::new(x, y, z))
    }

    /// `float4x4.EulerXZY(float, float, float)` @ 0x9350ac0.
    #[inline]
    pub fn euler_xzy_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_xzy(float3::new(x, y, z))
    }

    /// `float4x4.EulerYXZ(float, float, float)` @ 0x9350b10.
    #[inline]
    pub fn euler_yxz_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_yxz(float3::new(x, y, z))
    }

    /// `float4x4.EulerYZX(float, float, float)` @ 0x9350b60.
    #[inline]
    pub fn euler_yzx_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_yzx(float3::new(x, y, z))
    }

    /// `float4x4.EulerZXY(float, float, float)` @ 0x9350bb0.
    #[inline]
    pub fn euler_zxy_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_zxy(float3::new(x, y, z))
    }

    /// `float4x4.EulerZYX(float, float, float)` @ 0x9350c00.
    #[inline]
    pub fn euler_zyx_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_zyx(float3::new(x, y, z))
    }

    /// `float4x4.OrthoOffCenter(left, right, bottom, top, near, far)` @ 0x9351790. The body takes
    /// one reciprocal per span, doubles the two scale terms and the depth term (the `-2.0f` at
    /// 0x18cf299fc) but leaves the depth translation at a single `1/(far-near)`. Its stores are
    /// column-major — the translation is the fourth column — while `from_elements` takes rows.
    #[inline]
    pub fn ortho_off_center(l: f32, r: f32, b: f32, t: f32, near: f32, far: f32) -> Self {
        let sx = 1.0 / (r - l);
        let sy = 1.0 / (t - b);
        let sz = 1.0 / (far - near);
        Self::from_elements(
            sx + sx,
            0.0,
            0.0,
            -(l + r) * sx,
            0.0,
            sy + sy,
            0.0,
            -(t + b) * sy,
            0.0,
            0.0,
            sz * -2.0,
            -(far + near) * sz,
            0.0,
            0.0,
            0.0,
            1.0,
        )
    }
}

/// The `float4x4` Euler bodies store the `float3x3` products with `0.0` below each column and
/// `1.0` in the last element.
fn embed3x3(m: float3x3) -> float4x4 {
    float4x4::new(
        float4::new(m.c0.x, m.c0.y, m.c0.z, 0.0),
        float4::new(m.c1.x, m.c1.y, m.c1.z, 0.0),
        float4::new(m.c2.x, m.c2.y, m.c2.z, 0.0),
        float4::new(0.0, 0.0, 0.0, 1.0),
    )
}

// ---------------------------------------------------------------------------
// quaternion(float3x3) / quaternion(float4x4) — the branchless matrix→quaternion conversion
// src: quaternion..ctor(float3x3) @ 0x93757a0, quaternion..ctor(float4x4) @ 0x9375aa0
//      (`evidence/decompiled/Unity.Mathematics.dll.c`). The same formulas appear line-for-line in
//      the official `Unity.Mathematics/quaternion.cs`, which is the second ground truth here; the
//      component indices below are the ones each body actually reads, and they differ between the
//      two shapes because a `float3x3` column is three floats and a `float4x4` column is four.
// ---------------------------------------------------------------------------

/// One lane of the body's `sign_flips` vector: `base ^ (u_mask & U) ^ (t_mask & T)`, with the three
/// constant vectors the source writes out (`+---, +++-, ++-+, +-++`).
#[inline]
fn sign_flip(i: usize, u_mask: u32, t_mask: u32) -> u32 {
    const BASE: [u32; 4] = [0x0000_0000, 0x8000_0000, 0x8000_0000, 0x8000_0000];
    const U: [u32; 4] = [0x0000_0000, 0x8000_0000, 0x0000_0000, 0x8000_0000];
    const T: [u32; 4] = [0x8000_0000, 0x8000_0000, 0x8000_0000, 0x0000_0000];
    BASE[i] ^ (u_mask & U[i]) ^ (t_mask & T[i])
}

/// The shared body of both ctors. `u`, `v`, `w` are the three source **columns**, each read as
/// `[x, y, z, …]`; the fourth element of each is only used by the `float4x4` shape.
fn quaternion_from_columns(u: [f32; 4], v: [f32; 4], w: [f32; 4]) -> quaternion {
    let u_sign = u[0].to_bits() & 0x8000_0000;
    let t = v[1] + f32::from_bits(w[2].to_bits() ^ u_sign);
    // `(int)u_sign >> 31` and `(int)asint(t) >> 31`: an all-ones mask when the value is negative.
    let u_mask = ((u_sign as i32) >> 31) as u32;
    let t_mask = ((t.to_bits() as i32) >> 31) as u32;
    let tr = 1.0f32 + u[0].abs();

    let a = [tr, u[1], w[0], v[2]];
    let b = [t, v[0], u[2], w[1]];
    let mut value = [0.0f32; 4];
    for i in 0..4 {
        value[i] = a[i] + f32::from_bits(b[i].to_bits() ^ sign_flip(i, u_mask, t_mask));
    }

    // `value = (asuint(value) & ~u_mask) | (asuint(value.zwxy) & u_mask)`
    let bits = [
        value[0].to_bits(),
        value[1].to_bits(),
        value[2].to_bits(),
        value[3].to_bits(),
    ];
    let sel = [
        (bits[0] & !u_mask) | (bits[2] & u_mask),
        (bits[1] & !u_mask) | (bits[3] & u_mask),
        (bits[2] & !u_mask) | (bits[0] & u_mask),
        (bits[3] & !u_mask) | (bits[1] & u_mask),
    ];
    // `value = (asuint(value.wzyx) & ~t_mask) | (asuint(value) & t_mask)`
    let out = [
        (sel[3] & !t_mask) | (sel[0] & t_mask),
        (sel[2] & !t_mask) | (sel[1] & t_mask),
        (sel[1] & !t_mask) | (sel[2] & t_mask),
        (sel[0] & !t_mask) | (sel[3] & t_mask),
    ];
    let value = float4::new(
        f32::from_bits(out[0]),
        f32::from_bits(out[1]),
        f32::from_bits(out[2]),
        f32::from_bits(out[3]),
    );
    // The body inlines `normalize(value)` as `1.0f / sqrt(y² + x² + z² + w²)` times the value —
    // that lane order, and no zero guard.
    let s = (value.y * value.y + value.x * value.x + value.z * value.z + value.w * value.w).sqrt();
    quaternion::from_float4((1.0f32 / s) * value)
}

impl quaternion {
    /// `quaternion(float3x3 m)` @ 0x93757a0 — the unit quaternion of an orthonormal 3×3.
    #[inline]
    pub fn from_matrix3(m: float3x3) -> Self {
        quaternion_from_columns(
            [m.c0.x, m.c0.y, m.c0.z, 0.0],
            [m.c1.x, m.c1.y, m.c1.z, 0.0],
            [m.c2.x, m.c2.y, m.c2.z, 0.0],
        )
    }

    /// `quaternion(float4x4 m)` @ 0x9375aa0 — the same conversion over the first three columns of a
    /// 4×4; the fourth column and the bottom row are not read.
    #[inline]
    pub fn from_matrix4(m: float4x4) -> Self {
        quaternion_from_columns(
            [m.c0.x, m.c0.y, m.c0.z, m.c0.w],
            [m.c1.x, m.c1.y, m.c1.z, m.c1.w],
            [m.c2.x, m.c2.y, m.c2.z, m.c2.w],
        )
    }
}

// ---------------------------------------------------------------------------
// LookRotationSafe
// src: float3x3.LookRotationSafe @ 0x9339fc0, quaternion.LookRotationSafe @ 0x93761d0
// ---------------------------------------------------------------------------

/// `math.kEpsilon` — the lower bound of the bodies' validity test, read from VA 0x18cf28588
/// (1.0000000180025095e-35).
const K_EPSILON: f32 = f32::from_bits(0x0554_ad2e);
/// `math.kMaxFloat` — the upper bound, read from VA 0x18cf295b0 (1.0000000409184788e+35).
const K_MAX_FLOAT: f32 = f32::from_bits(0x799a_130c);

/// The bodies' own min/max fold: `if !isnan(b) && b <= a { a = b }`, applied left to right. This is
/// *not* `math.min` — with `b` NaN and `a` finite the fold keeps `a`, where `math.min` returns `b`.
#[inline]
fn fold_min(a: f32, b: f32) -> f32 {
    if !b.is_nan() && b <= a {
        b
    } else {
        a
    }
}

#[inline]
fn fold_max(a: f32, b: f32) -> f32 {
    if !b.is_nan() && a <= b {
        b
    } else {
        a
    }
}

/// The part both bodies share: the normalised forward vector, the normalised `cross(up, forward)`
/// and whether the shipped validity test passes. Squared lengths are summed `y, x, z` because that
/// is the lane order the bodies use.
fn look_basis(forward: float3, up: float3) -> (float3, float3, bool) {
    let len2_f = forward.y * forward.y + forward.x * forward.x + forward.z * forward.z;
    let len2_u = up.y * up.y + up.x * up.x + up.z * up.z;
    let inv_f = 1.0 / len2_f.sqrt();
    let f = float3::new(forward.x * inv_f, forward.y * inv_f, forward.z * inv_f);
    let inv_u = 1.0 / len2_u.sqrt();
    let u = float3::new(up.x * inv_u, up.y * inv_u, up.z * inv_u);

    let rx = u.y * f.z - u.z * f.y;
    let ry = u.z * f.x - u.x * f.z;
    let rz = u.x * f.y - u.y * f.x;
    let len2_r = ry * ry + rx * rx + rz * rz;
    let inv_r = 1.0 / len2_r.sqrt();
    let r = float3::new(rx * inv_r, ry * inv_r, rz * inv_r);

    let mut lo = len2_f;
    lo = fold_min(lo, len2_u);
    lo = fold_min(lo, len2_r);
    let mut hi = len2_f;
    hi = fold_max(hi, len2_u);
    hi = fold_max(hi, len2_r);
    let degenerate = (lo <= K_EPSILON || K_MAX_FLOAT <= hi)
        || (f32::INFINITY <= len2_f.abs() || f32::INFINITY <= len2_u.abs());
    let valid = !degenerate && len2_r.abs() < f32::INFINITY;
    (f, r, valid)
}

impl float3x3 {
    /// `float3x3.LookRotationSafe(float3 forward, float3 up)` @ 0x9339fc0 — the basis
    /// `(right, cross(forward, right), forward)` with `right = normalize(cross(up, forward))`, and
    /// the identity when any of the three squared lengths fails the bodies' test. Unlike
    /// `look_rotation` this never divides by a zero-length projection, so it is what the game calls
    /// when a direction might be degenerate.
    #[inline]
    pub fn look_rotation_safe(forward: float3, up: float3) -> Self {
        let (f, r, valid) = look_basis(forward, up);
        if !valid {
            return Self::identity;
        }
        Self::new(
            r,
            float3::new(
                r.z * f.y - r.y * f.z,
                r.x * f.z - r.z * f.x,
                r.y * f.x - r.x * f.y,
            ),
            f,
        )
    }
}

impl quaternion {
    /// `quaternion.LookRotationSafe(float3 forward, float3 up)` @ 0x93761d0 — the same basis fed
    /// through `quaternion(float3x3)`, or `quaternion.identity` when the basis is degenerate. The
    /// fallback matters: with a zero-length input the basis the body computed is already NaN (the
    /// normalisation divides by `sqrt(0)`), so converting it would give a NaN quaternion, and the
    /// body selects the identity literal instead.
    #[inline]
    pub fn look_rotation_safe(forward: float3, up: float3) -> Self {
        let (f, r, valid) = look_basis(forward, up);
        if !valid {
            return Self::identity;
        }
        Self::from_matrix3(float3x3::new(
            r,
            float3::new(
                r.z * f.y - r.y * f.z,
                r.x * f.z - r.z * f.x,
                r.y * f.x - r.x * f.y,
            ),
            f,
        ))
    }
}
