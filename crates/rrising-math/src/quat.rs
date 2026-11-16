//! `Unity.Mathematics.quaternion` — a `float4` value in `x, y, z, w` order (IL2CPP layout:
//! `public float4 value; // 0x0`).
//!
//! The multiplication order, the half-angle construction and the `mul(quaternion, float3)` form are
//! the original's, kept operation-for-operation because rotation results feed positions and the game
//! compares them across a network.

use crate::vector::{float3, float4};

#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[repr(C)]
pub struct quaternion {
    pub value: float4,
}

impl quaternion {
    pub const identity: Self = Self {
        value: float4 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
    };

    #[inline]
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self {
            value: float4::new(x, y, z, w),
        }
    }

    #[inline]
    pub const fn from_float4(v: float4) -> Self {
        Self { value: v }
    }

    #[inline]
    pub fn xyz(&self) -> float3 {
        float3::new(self.value.x, self.value.y, self.value.z)
    }

    /// `quaternion.RotateX(float angle)` — half-angle about +X.
    #[inline]
    pub fn rotate_x(angle: f32) -> Self {
        Self::new((0.5 * angle).sin(), 0.0, 0.0, (0.5 * angle).cos())
    }

    #[inline]
    pub fn rotate_y(angle: f32) -> Self {
        Self::new(0.0, (0.5 * angle).sin(), 0.0, (0.5 * angle).cos())
    }

    #[inline]
    pub fn rotate_z(angle: f32) -> Self {
        Self::new(0.0, 0.0, (0.5 * angle).sin(), (0.5 * angle).cos())
    }

    /// `quaternion.AxisAngle(float3 axis, float angle)`; the caller passes a unit axis.
    #[inline]
    pub fn axis_angle(axis: float3, angle: f32) -> Self {
        let sa = (0.5 * angle).sin();
        let ca = (0.5 * angle).cos();
        Self::new(axis.x * sa, axis.y * sa, axis.z * sa, ca)
    }

    /// `quaternion.conjugate` — negates the vector part.
    #[inline]
    pub fn conjugate(q: Self) -> Self {
        Self::new(-q.value.x, -q.value.y, -q.value.z, q.value.w)
    }

    #[inline]
    pub fn dot(a: Self, b: Self) -> f32 {
        a.value.dot(b.value)
    }

    #[inline]
    pub fn lengthsq(q: Self) -> f32 {
        q.value.dot(q.value)
    }

    #[inline]
    pub fn length(q: Self) -> f32 {
        Self::lengthsq(q).sqrt()
    }

    /// `quaternion.normalize` — `1.0f / sqrt(y² + x² + z² + w²)` times the value, in that lane
    /// order and with **no zero guard**: the inlined copy inside `quaternion(float3x3)`
    /// @ 0x93757a0 is unguarded, and the guarded form is a different function
    /// (`math.normalizesafe`). A zero-length input therefore yields infinities here, as it does in
    /// the original.
    #[inline]
    pub fn normalize(q: Self) -> Self {
        let v = q.value;
        let s = (v.y * v.y + v.x * v.x + v.z * v.z + v.w * v.w).sqrt();
        Self::from_float4((1.0f32 / s) * v)
    }

    /// `quaternion.inverse` — conjugate over the squared length.
    #[inline]
    pub fn inverse(q: Self) -> Self {
        let c = Self::conjugate(q);
        Self::from_float4(c.value / Self::lengthsq(q))
    }

    /// `quaternion.mul(quaternion left, float3 right)` — the original rotates with
    /// `right + 2 * cross(qv, cross(qv, right) + qw * right)` written as the two-cross form.
    #[inline]
    pub fn mul_qv(left: Self, right: float3) -> float3 {
        let qv = left.xyz();
        let t = qv.cross(right) * 2.0;
        right + t * left.value.w + qv.cross(t)
    }

    /// `Transform` is the same rotation applied to a point; the game uses both spellings.
    #[inline]
    pub fn transform(left: Self, point: float3) -> float3 {
        Self::mul_qv(left, point)
    }

    /// `quaternion.RotateNormalX/Y/Z` rotate a basis vector without the translation term.
    #[inline]
    pub fn rotate_normal_x(q: Self) -> float3 {
        Self::mul_qv(q, float3::new(1.0, 0.0, 0.0))
    }
    #[inline]
    pub fn rotate_normal_y(q: Self) -> float3 {
        Self::mul_qv(q, float3::new(0.0, 1.0, 0.0))
    }
    #[inline]
    pub fn rotate_normal_z(q: Self) -> float3 {
        Self::mul_qv(q, float3::new(0.0, 0.0, 1.0))
    }

    /// `quaternion.lerp` — component-wise, choosing the shorter arc by the dot sign.
    #[inline]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let dot = Self::dot(a, b);
        let scale = if dot < 0.0 { -1.0 } else { 1.0 };
        Self::from_float4(a.value + (b.value * scale - a.value) * t)
    }

    /// `quaternion.slerp` — the original's short-path spherical interpolation.
    #[inline]
    pub fn slerp(a: Self, b: Self, t: f32) -> Self {
        let mut bb = b;
        let mut dot = Self::dot(a, b);
        if dot < 0.0 {
            bb = Self::from_float4(-b.value);
            dot = -dot;
        }
        if dot > 0.9995 {
            // The original falls back to lerp and normalizes when the arc is degenerate.
            return Self::normalize(Self::lerp(a, bb, t));
        }
        let theta = dot.min(1.0).acos();
        let s = (1.0 - t) * theta;
        let c = s.cos();
        let sn = ((1.0 - t) * theta).sin();
        let denom = (1.0 - dot * dot).sqrt();
        let w2 = if denom > 1e-8 { sn / denom } else { sn };
        Self::from_float4(a.value * c + bb.value * w2)
    }
}

impl std::ops::Mul for quaternion {
    type Output = quaternion;
    /// `quaternion * quaternion` — the original's component order, term for term.
    #[inline]
    fn mul(self, r: Self) -> Self {
        let va = self.value;
        let vb = r.value;
        Self::new(
            va.w * vb.x + va.x * vb.w + (va.y * vb.z - va.z * vb.y),
            va.w * vb.y + va.y * vb.w + (va.z * vb.x - va.x * vb.z),
            va.w * vb.z + va.z * vb.w + (va.x * vb.y - va.y * vb.x),
            va.w * vb.w - (va.x * vb.x + va.y * vb.y + va.z * vb.z),
        )
    }
}

impl std::ops::Mul<f32> for quaternion {
    type Output = quaternion;
    #[inline]
    fn mul(self, r: f32) -> Self {
        Self::from_float4(self.value * r)
    }
}

impl std::ops::Add for quaternion {
    type Output = quaternion;
    #[inline]
    fn add(self, r: Self) -> Self {
        Self::from_float4(self.value + r.value)
    }
}

impl std::ops::Sub for quaternion {
    type Output = quaternion;
    #[inline]
    fn sub(self, r: Self) -> Self {
        Self::from_float4(self.value - r.value)
    }
}

impl std::ops::Neg for quaternion {
    type Output = quaternion;
    #[inline]
    fn neg(self) -> Self {
        Self::from_float4(-self.value)
    }
}

/// `quaternion.GetHashCode()` — the original hashes the four components; hashing their bit patterns
/// is the port's counterpart, the same choice the generated vector types make.
impl std::hash::Hash for quaternion {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

/// `quaternion.ToString()` — the shipped format string for this type, from
/// `evidence/math_tostring.tsv` (literal at VA 0xd9b6ef8): the name, then the four components with
/// an `f` after each, which is how the library's own generator prints a `float`.
impl std::fmt::Display for quaternion {
    fn fmt(&self, w: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            w,
            "quaternion({}f, {}f, {}f, {}f)",
            self.value.x, self.value.y, self.value.z, self.value.w
        )
    }
}

/// `quaternion.op_Implicit(float4 v)` — the construction the dump declares as an implicit
/// conversion. Rust spells it as `From`; see docs/PORT.md. The `Quaternion`-typed
/// `op_Implicit` pair needs `UnityEngine.Quaternion`, which is not ported yet.
impl From<float4> for quaternion {
    #[inline]
    fn from(v: float4) -> Self {
        Self::from_float4(v)
    }
}
