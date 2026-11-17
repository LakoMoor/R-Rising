//! `Unity.Mathematics.RigidTransform` — a `quaternion` rotation plus a `float3` translation, the
//! type every rigid placement in the game is stored in (`Unity.Transforms.LocalToWorld` decomposes
//! to it, `Aabb`/`Collider`/`Plane` helpers take it, and `ProjectM`'s character and projectile
//! systems build it per frame).
//!
//! Source of truth: the decompiled bodies in `evidence/decompiled/Unity.Mathematics.dll.c` —
//! `.ctor(float3x3, float3)` @ 0x938d870, `.ctor(float4x4)` @ 0x938d8e0, `AxisAngle` @ 0x938d960,
//! `EulerXYZ…EulerZYX` @ 0x938da30–0x938dd00, the scalar overloads @ 0x938dd90–0x938e100,
//! `Euler` @ 0x938e1b0 and 0x938e4b0, `RotateX/Y/Z` @ 0x938e500/0x938e5b0/0x938e660,
//! `Translate` @ 0x938e710, `Equals` @ 0x938e780, `GetHashCode` @ 0x938e920, `ToString` @ 0x938e9c0,
//! `.cctor` @ 0x938f040; the `math` wrappers @ 0x92cd330–0x92cde30; `float4x4(RigidTransform)`
//! @ 0x934fa60 and `math.float4x4(RigidTransform)` @ 0x92bae30.
//!
//! The `math` wrappers are inherent methods here because Rust has no overloading; each carries the
//! C# spelling in its doc comment. The fused `inverse`/`mul` bodies are transcribed operation for
//! operation — they fold the quaternion product and the vector rotation together, and the game
//! compares these transforms across a network, so the rounding order is observable.

use crate::matrix::{float3x3, float4x4};
use crate::quat::quaternion;
use crate::vector::{float3, float4, uint4};

#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[repr(C)]
pub struct RigidTransform {
    pub rot: quaternion,
    pub pos: float3,
}

impl RigidTransform {
    /// `.cctor` @ 0x938f040 — `identity`: the 16 bytes at VA 0x18cf2a5d0 (`(0, 0, 0, 1)`) over a
    /// zero translation.
    pub const identity: Self = Self {
        rot: quaternion::identity,
        pos: float3::zero,
    };

    /// `.ctor(quaternion rotation, float3 translation)` — a copy of both fields.
    #[inline]
    pub const fn new(rot: quaternion, pos: float3) -> Self {
        Self { rot, pos }
    }

    /// `.ctor(float3x3 rotation, float3 translation)` @ 0x938d870 — the rotation goes through
    /// `quaternion(float3x3)`.
    #[inline]
    pub fn from_matrix3(rotation: float3x3, translation: float3) -> Self {
        Self {
            rot: quaternion::from_matrix3(rotation),
            pos: translation,
        }
    }

    /// `.ctor(float4x4 transform)` @ 0x938d8e0 — `rot = new quaternion(transform)`,
    /// `pos = transform.c3.xyz`.
    #[inline]
    pub fn from_matrix4(transform: float4x4) -> Self {
        Self {
            rot: quaternion::from_matrix4(transform),
            pos: float3::new(transform.c3.x, transform.c3.y, transform.c3.z),
        }
    }

    /// `AxisAngle(float3 axis, float angle)` @ 0x938d960 — the half-angle rotation about `axis`
    /// with a zero translation, the caller passing a unit axis.
    #[inline]
    pub fn axis_angle(axis: float3, angle: f32) -> Self {
        Self {
            rot: quaternion::axis_angle(axis, angle),
            pos: float3::zero,
        }
    }

    /// `EulerXYZ(float3 xyz)` @ 0x938da30 — `quaternion.EulerXYZ` with a zero translation. The
    /// remaining orders are the same shape; only the quaternion differs.
    #[inline]
    pub fn euler_xyz(xyz: float3) -> Self {
        Self {
            rot: quaternion::euler_xyz(xyz),
            pos: float3::zero,
        }
    }

    #[inline]
    pub fn euler_xzy(xyz: float3) -> Self {
        Self {
            rot: quaternion::euler_xzy(xyz),
            pos: float3::zero,
        }
    }

    #[inline]
    pub fn euler_yxz(xyz: float3) -> Self {
        Self {
            rot: quaternion::euler_yxz(xyz),
            pos: float3::zero,
        }
    }

    #[inline]
    pub fn euler_yzx(xyz: float3) -> Self {
        Self {
            rot: quaternion::euler_yzx(xyz),
            pos: float3::zero,
        }
    }

    /// `EulerZXY(float3 xyz)` @ 0x938dc70 — the order `Euler` defaults to.
    #[inline]
    pub fn euler_zxy(xyz: float3) -> Self {
        Self {
            rot: quaternion::euler_zxy(xyz),
            pos: float3::zero,
        }
    }

    #[inline]
    pub fn euler_zyx(xyz: float3) -> Self {
        Self {
            rot: quaternion::euler_zyx(xyz),
            pos: float3::zero,
        }
    }

    #[inline]
    pub fn euler_xyz_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_xyz(float3::new(x, y, z))
    }

    #[inline]
    pub fn euler_xzy_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_xzy(float3::new(x, y, z))
    }

    #[inline]
    pub fn euler_yxz_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_yxz(float3::new(x, y, z))
    }

    #[inline]
    pub fn euler_yzx_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_yzx(float3::new(x, y, z))
    }

    #[inline]
    pub fn euler_zxy_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_zxy(float3::new(x, y, z))
    }

    #[inline]
    pub fn euler_zyx_3(x: f32, y: f32, z: f32) -> Self {
        Self::euler_zyx(float3::new(x, y, z))
    }

    /// `Euler(float3 xyz, math.RotationOrder order = 4)` @ 0x938e1b0 — the body's switch over the
    /// six orders, `ZXY` being 4. Its `default:` arm returns `identity`; Rust's `RotationOrder`
    /// cannot hold a value outside 0..5, so that arm is unreachable here.
    #[inline]
    pub fn euler(xyz: float3, order: crate::RotationOrder) -> Self {
        Self {
            rot: quaternion::euler_order(xyz, order),
            pos: float3::zero,
        }
    }

    /// `Euler(float x, float y, float z, math.RotationOrder order = 4)` @ 0x938e4b0.
    #[inline]
    pub fn euler_3(x: f32, y: f32, z: f32, order: crate::RotationOrder) -> Self {
        Self::euler(float3::new(x, y, z), order)
    }

    /// `RotateX(float angle)` @ 0x938e500 — a half-angle rotation about +X, zero translation.
    #[inline]
    pub fn rotate_x(angle: f32) -> Self {
        Self {
            rot: quaternion::rotate_x(angle),
            pos: float3::zero,
        }
    }

    #[inline]
    pub fn rotate_y(angle: f32) -> Self {
        Self {
            rot: quaternion::rotate_y(angle),
            pos: float3::zero,
        }
    }

    #[inline]
    pub fn rotate_z(angle: f32) -> Self {
        Self {
            rot: quaternion::rotate_z(angle),
            pos: float3::zero,
        }
    }

    /// `Translate(float3 vector)` @ 0x938e710 — the identity rotation with `vector` as its
    /// translation.
    #[inline]
    pub fn translate(vector: float3) -> Self {
        Self {
            rot: quaternion::identity,
            pos: vector,
        }
    }

    /// `math.inverse(RigidTransform a)` @ 0x92cd480. The body fuses the conjugate-over-normed
    /// rotation with the rotated negated translation: the reciprocal is `1.0f / (y² + x² + z² +
    /// w²)` in that lane order, the vector part is negated by *multiplying by −1.0f* (so a zero
    /// component comes out as −0.0), and the translation is the two-cross form applied to
    /// `−a.pos`.
    #[inline]
    pub fn inverse(a: Self) -> Self {
        let q = a.rot.value;
        let n = 1.0f32 / (q.y * q.y + q.x * q.x + q.z * q.z + q.w * q.w);
        let iw = q.w * n;
        let ix = q.x * n * -1.0f32;
        let iy = q.y * n * -1.0f32;
        let iz = q.z * n * -1.0f32;
        let px = -a.pos.x;
        let py = -a.pos.y;
        let pz = -a.pos.z;
        let mut t1 = py * ix - px * iy;
        t1 += t1;
        let mut t2 = pz * iy - py * iz;
        t2 += t2;
        let mut t3 = px * iz - pz * ix;
        t3 += t3;
        Self {
            rot: quaternion::new(ix, iy, iz, iw),
            pos: float3::new(
                (t1 * iy - t3 * iz) + t2 * iw + px,
                (t2 * iz - t1 * ix) + t3 * iw + py,
                (t3 * ix - t2 * iy) + t1 * iw + pz,
            ),
        }
    }

    /// `math.mul(RigidTransform a, RigidTransform b)` @ 0x92cd670 — `a.rot * b.rot` in the body's
    /// term order, then `rotate(a.rot, b.pos) + a.pos` fused with the same products.
    #[inline]
    pub fn mul(a: Self, b: Self) -> Self {
        let x = a.rot.value;
        let y = b.rot.value;
        let mut t1 = x.x * b.pos.y - x.y * b.pos.x;
        t1 += t1;
        let mut t2 = x.y * b.pos.z - x.z * b.pos.y;
        t2 += t2;
        let mut t3 = x.z * b.pos.x - x.x * b.pos.z;
        t3 += t3;
        let neg = (y.y * x.y + y.x * x.x) * -1.0f32;
        Self {
            rot: quaternion::new(
                (y.z * x.y + y.w * x.x + y.x * x.w) - y.y * x.z,
                (y.x * x.z + y.w * x.y + y.y * x.w) - y.z * x.x,
                (y.w * x.z + y.y * x.x + y.z * x.w) - y.x * x.y,
                (neg + y.w * x.w) - y.z * x.z,
            ),
            pos: float3::new(
                (t1 * x.y - t3 * x.z) + t2 * x.w + b.pos.x + a.pos.x,
                (t2 * x.z - t1 * x.x) + t3 * x.w + b.pos.y + a.pos.y,
                (t3 * x.x - t2 * x.y) + t1 * x.w + b.pos.z + a.pos.z,
            ),
        }
    }

    /// `math.mul(RigidTransform a, float4 pos)` @ 0x92cd920 — the rotation applied to `pos.xyz`
    /// with `pos.w` scaling the translation, and `pos.w` passed through unchanged.
    #[inline]
    pub fn mul_v4(a: Self, p: float4) -> float4 {
        let q = a.rot.value;
        let mut t1 = p.y * q.x - p.x * q.y;
        t1 += t1;
        let mut t2 = p.z * q.y - p.y * q.z;
        t2 += t2;
        let mut t3 = p.x * q.z - p.z * q.x;
        t3 += t3;
        float4::new(
            (q.y * t1 - q.z * t3) + q.w * t2 + p.x + p.w * a.pos.x,
            (q.z * t2 - q.x * t1) + q.w * t3 + p.y + p.w * a.pos.y,
            (q.x * t3 - q.y * t2) + q.w * t1 + p.z + p.w * a.pos.z,
            p.w,
        )
    }

    /// `math.rotate(RigidTransform a, float3 dir)` @ 0x92cdac0 — the rotation only, no translation.
    #[inline]
    pub fn rotate(a: Self, dir: float3) -> float3 {
        let q = a.rot.value;
        let mut t1 = q.x * dir.y - q.y * dir.x;
        t1 += t1;
        let mut t2 = q.y * dir.z - q.z * dir.y;
        t2 += t2;
        let mut t3 = q.z * dir.x - q.x * dir.z;
        t3 += t3;
        float3::new(
            (t1 * q.y - t3 * q.z) + t2 * q.w + dir.x,
            (t2 * q.z - t1 * q.x) + t3 * q.w + dir.y,
            (t3 * q.x - t2 * q.y) + t1 * q.w + dir.z,
        )
    }

    /// `math.transform(RigidTransform a, float3 pos)` @ 0x92cdc10 — `rotate` plus the translation.
    #[inline]
    pub fn transform(a: Self, pos: float3) -> float3 {
        let q = a.rot.value;
        let mut t1 = q.x * pos.y - q.y * pos.x;
        t1 += t1;
        let mut t2 = q.y * pos.z - q.z * pos.y;
        t2 += t2;
        let mut t3 = q.z * pos.x - q.x * pos.z;
        t3 += t3;
        float3::new(
            (t1 * q.y - t3 * q.z) + t2 * q.w + pos.x + a.pos.x,
            (t2 * q.z - t1 * q.x) + t3 * q.w + pos.y + a.pos.y,
            (t3 * q.x - t2 * q.y) + t1 * q.w + pos.z + a.pos.z,
        )
    }

    /// `math.float4x4(RigidTransform transform)` @ 0x92bae30, which calls
    /// `float4x4(RigidTransform)` @ 0x934fa60: the rotation matrix embedded column by column with
    /// `0.0` below each column and the translation as the fourth column.
    #[inline]
    pub fn to_float4x4(a: Self) -> float4x4 {
        let m = float3x3::from_quaternion(a.rot);
        float4x4::new(
            float4::new(m.c0.x, m.c0.y, m.c0.z, 0.0),
            float4::new(m.c1.x, m.c1.y, m.c1.z, 0.0),
            float4::new(m.c2.x, m.c2.y, m.c2.z, 0.0),
            float4::new(a.pos.x, a.pos.y, a.pos.z, 1.0),
        )
    }
}

/// `math.RigidTransform(quaternion rot, float3 pos)` @ 0x92cd330.
#[inline]
pub fn rigid_transform(rot: quaternion, pos: float3) -> RigidTransform {
    RigidTransform::new(rot, pos)
}

/// `math.RigidTransform(float3x3 rotation, float3 translation)` @ 0x92cd350.
#[inline]
pub fn rigid_transform_m3(rotation: float3x3, translation: float3) -> RigidTransform {
    RigidTransform::from_matrix3(rotation, translation)
}

/// `math.RigidTransform(float4x4 transform)` @ 0x92cd3d0.
#[inline]
pub fn rigid_transform_m4(transform: float4x4) -> RigidTransform {
    RigidTransform::from_matrix4(transform)
}

/// `math.hash(RigidTransform t)` @ 0x92cdd90 — the same mix as `GetHashCode` @ 0x938e920: every
/// component's bits multiplied by a prime and summed left to right in the body's order.
#[inline]
pub fn hash_rigid(t: RigidTransform) -> u32 {
    let q = t.rot.value;
    let p = t.pos;
    p.y.to_bits()
        .wrapping_mul(0x4462_9891)
        .wrapping_add(q.w.to_bits().wrapping_mul(0x43be_0b51))
        .wrapping_add(p.x.to_bits().wrapping_mul(0xf3a3_52ef))
        .wrapping_add(q.x.to_bits().wrapping_mul(0xe696_26ff))
        .wrapping_add(q.y.to_bits().wrapping_mul(0xbd01_0eeb))
        .wrapping_add(p.z.to_bits().wrapping_mul(0xb1ac_a801))
        .wrapping_add(q.z.to_bits().wrapping_mul(0x9ce_de1d1))
        .wrapping_add(0xb27f_b9d4)
}

/// `math.hashwide(RigidTransform t)` @ 0x92cde30 — four lanes, each its own pair of multipliers
/// plus the shared salt. Note the third lane reuses the *translation x* component, and the fourth
/// pairs `pos.z` with `rot.w`; that is what the body does.
#[inline]
pub fn hashwide_rigid(t: RigidTransform) -> uint4 {
    let q = t.rot.value;
    let p = t.pos;
    uint4::new(
        p.x.to_bits()
            .wrapping_mul(0x884b_d90f)
            .wrapping_add(q.x.to_bits().wrapping_mul(0xb130_c137))
            .wrapping_add(0x189a_d918),
        q.y.to_bits()
            .wrapping_mul(0x5483_4775)
            .wrapping_add(p.y.to_bits().wrapping_mul(0xe529_3d0d))
            .wrapping_add(0x189a_d918),
        q.z.to_bits()
            .wrapping_mul(0x7c02_2221)
            .wrapping_add(p.x.to_bits().wrapping_mul(0x5efb_ba2d))
            .wrapping_add(0x189a_d918),
        p.z.to_bits()
            .wrapping_mul(0x5efb_ba2d)
            .wrapping_add(q.w.to_bits().wrapping_mul(0xa2d0_0edf))
            .wrapping_add(0x189a_d918),
    )
}

/// `RigidTransform.GetHashCode()` @ 0x938e920 — the same mix `math.hash` uses, so the two agree by
/// construction rather than by convention.
impl std::hash::Hash for RigidTransform {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(hash_rigid(*self));
    }
}

/// `RigidTransform.ToString()` @ 0x938e9c0. The format string is the literal at VA 0x18dab29d8,
/// which `stringliteral.json` gives as `RigidTransform(({0}f, {1}f, {2}f, {3}f),  ({4}f, {5}f,
/// {6}f))` — note the two spaces before the translation group and the `f` after each number.
impl std::fmt::Display for RigidTransform {
    fn fmt(&self, w: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let q = self.rot.value;
        write!(
            w,
            "RigidTransform(({}f, {}f, {}f, {}f),  ({}f, {}f, {}f))",
            q.x, q.y, q.z, q.w, self.pos.x, self.pos.y, self.pos.z
        )
    }
}

/// `math.mul(RigidTransform, RigidTransform)` as an operator, for ported systems that read `a * b`.
impl std::ops::Mul for RigidTransform {
    type Output = Self;
    #[inline]
    fn mul(self, rhs: Self) -> Self {
        RigidTransform::mul(self, rhs)
    }
}
