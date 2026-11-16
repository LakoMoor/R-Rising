//! `Unity.Mathematics.float3x3` and `float4x4`.
//!
//! Both are stored **column-major** exactly as IL2CPP lays them out (`float3x3`: `c0 // 0x0`,
//! `c1 // 0xC`, `c2 // 0x18`; `float4x4`: `c0..c3` at 0x0/0x10/0x20/0x30), so a matrix can be
//! copied straight into a uniform buffer the way the original uploads it.
//!
//! `determinant` and `inverse` are written as a readable cofactor expansion. The original unrolls
//! them through 2x2 minors, which changes only the float rounding order; that is recorded in
//! `docs/PORT.md` as a known deviation to re-check against the decompiled body.

use crate::quat::quaternion;
use crate::vector::{float3, float4};

#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[repr(C)]
pub struct float3x3 {
    pub c0: float3,
    pub c1: float3,
    pub c2: float3,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[repr(C)]
pub struct float4x4 {
    pub c0: float4,
    pub c1: float4,
    pub c2: float4,
    pub c3: float4,
}

/// Determinant of the 3x3 whose columns are `p, q, r` restricted to rows `i0, i1, i2`.
fn det3(p: [f32; 4], q: [f32; 4], r: [f32; 4], i0: usize, i1: usize, i2: usize) -> f32 {
    let (a0, a1, a2) = (p[i0], p[i1], p[i2]);
    let (b0, b1, b2) = (q[i0], q[i1], q[i2]);
    let (c0, c1, c2) = (r[i0], r[i1], r[i2]);
    a0 * (b1 * c2 - b2 * c1) - b0 * (a1 * c2 - a2 * c1) + c0 * (a1 * b2 - a2 * b1)
}

fn comp(v: float3, i: usize) -> f32 {
    [v.x, v.y, v.z][i]
}

fn comp4(v: float4, i: usize) -> f32 {
    [v.x, v.y, v.z, v.w][i]
}

impl float3x3 {
    pub const identity: Self = Self {
        c0: float3::new(1.0, 0.0, 0.0),
        c1: float3::new(0.0, 1.0, 0.0),
        c2: float3::new(0.0, 0.0, 1.0),
    };

    /// `new(float3 c0, float3 c1, float3 c2)` — the arguments are columns.
    #[inline]
    pub const fn new(c0: float3, c1: float3, c2: float3) -> Self {
        Self { c0, c1, c2 }
    }

    /// `new(m00, m01, m02, m10, m11, m12, m20, m21, m22)` — **row-major arguments**, the order the
    /// original's constructor declares (`float3x3.gen.cs` sets `c0 = (m00, m10, m20)`).
    #[inline]
    pub const fn from_elements(
        m00: f32,
        m01: f32,
        m02: f32,
        m10: f32,
        m11: f32,
        m12: f32,
        m20: f32,
        m21: f32,
        m22: f32,
    ) -> Self {
        Self {
            c0: float3::new(m00, m10, m20),
            c1: float3::new(m01, m11, m21),
            c2: float3::new(m02, m12, m22),
        }
    }

    /// Row `i`, as the original's `rowget(int)` exposes it.
    #[inline]
    pub fn row(&self, i: usize) -> float3 {
        float3::new(comp(self.c0, i), comp(self.c1, i), comp(self.c2, i))
    }

    #[inline]
    pub fn transpose(&self) -> Self {
        Self::new(self.row(0), self.row(1), self.row(2))
    }

    /// `mul(float3x3 a, float3x3 b)`: column `j` of the result is `a` applied to column `j` of `b`.
    #[inline]
    pub fn mul_matrix(a: Self, b: Self) -> Self {
        Self::new(a * b.c0, a * b.c1, a * b.c2)
    }

    /// `float3x3(quaternion)` — the rotation basis the original builds.
    #[inline]
    pub fn from_quaternion(q: quaternion) -> Self {
        let v = q.value;
        let x2 = v.x + v.x;
        let y2 = v.y + v.y;
        let z2 = v.z + v.z;
        let xx = v.x * x2;
        let xy = v.x * y2;
        let xz = v.x * z2;
        let yy = v.y * y2;
        let yz = v.y * z2;
        let zz = v.z * z2;
        let wx = v.w * x2;
        let wy = v.w * y2;
        let wz = v.w * z2;
        // Columns as `float3x3(quaternion)` derives them; `from_elements` takes rows, so they are
        // written transposed here.
        Self::new(
            float3::new(1.0 - (yy + zz), xy + wz, xz - wy),
            float3::new(xy - wz, 1.0 - (xx + zz), yz + wx),
            float3::new(xz + wy, yz - wx, 1.0 - (xx + yy)),
        )
    }

    #[inline]
    pub fn determinant(&self) -> f32 {
        det3(
            [self.c0.x, self.c0.y, self.c0.z, 0.0],
            [self.c1.x, self.c1.y, self.c1.z, 0.0],
            [self.c2.x, self.c2.y, self.c2.z, 0.0],
            0,
            1,
            2,
        )
    }

    /// `scale(float3)` scales the columns, which is the original's `Scale`.
    #[inline]
    pub fn scale(&self, s: float3) -> Self {
        Self::new(self.c0 * s.x, self.c1 * s.y, self.c2 * s.z)
    }

    /// `math.normalizematrix`-style column normalization used for the rotation part of a transform.
    #[inline]
    pub fn normalize_columns(&self) -> Self {
        let n = |v: float3| {
            let l = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
            if l > 0.0 {
                v / l
            } else {
                v
            }
        };
        Self::new(n(self.c0), n(self.c1), n(self.c2))
    }
}

impl float4x4 {
    pub const identity: Self = Self {
        c0: float4::new(1.0, 0.0, 0.0, 0.0),
        c1: float4::new(0.0, 1.0, 0.0, 0.0),
        c2: float4::new(0.0, 0.0, 1.0, 0.0),
        c3: float4::new(0.0, 0.0, 0.0, 1.0),
    };

    #[inline]
    pub const fn new(c0: float4, c1: float4, c2: float4, c3: float4) -> Self {
        Self { c0, c1, c2, c3 }
    }

    /// **Row-major arguments**, matching `float4x4.gen.cs:69` (`c0 = new float4(m00, m10, m20, m30)`).
    #[inline]
    #[allow(clippy::too_many_arguments)] // the original's constructor takes the sixteen elements
    pub const fn from_elements(
        m00: f32,
        m01: f32,
        m02: f32,
        m03: f32,
        m10: f32,
        m11: f32,
        m12: f32,
        m13: f32,
        m20: f32,
        m21: f32,
        m22: f32,
        m23: f32,
        m30: f32,
        m31: f32,
        m32: f32,
        m33: f32,
    ) -> Self {
        Self {
            c0: float4::new(m00, m10, m20, m30),
            c1: float4::new(m01, m11, m21, m31),
            c2: float4::new(m02, m12, m22, m32),
            c3: float4::new(m03, m13, m23, m33),
        }
    }

    #[inline]
    pub fn row(&self, i: usize) -> float4 {
        float4::new(
            comp4(self.c0, i),
            comp4(self.c1, i),
            comp4(self.c2, i),
            comp4(self.c3, i),
        )
    }

    #[inline]
    pub fn transpose(&self) -> Self {
        Self::new(self.row(0), self.row(1), self.row(2), self.row(3))
    }

    /// `mul(float4x4 a, float4x4 b)` — `a` applied to each column of `b`.
    #[inline]
    pub fn mul_matrix(a: Self, b: Self) -> Self {
        Self::new(a * b.c0, a * b.c1, a * b.c2, a * b.c3)
    }

    /// `float4x4.Translate(float3)` — translation lives in the fourth column.
    #[inline]
    pub fn translate(t: float3) -> Self {
        Self::new(
            float4::new(1.0, 0.0, 0.0, 0.0),
            float4::new(0.0, 1.0, 0.0, 0.0),
            float4::new(0.0, 0.0, 1.0, 0.0),
            float4::new(t.x, t.y, t.z, 1.0),
        )
    }

    #[inline]
    pub fn scale(s: float3) -> Self {
        Self::new(
            float4::new(s.x, 0.0, 0.0, 0.0),
            float4::new(0.0, s.y, 0.0, 0.0),
            float4::new(0.0, 0.0, s.z, 0.0),
            float4::new(0.0, 0.0, 0.0, 1.0),
        )
    }

    #[inline]
    pub fn rotate(q: quaternion) -> Self {
        let m = float3x3::from_quaternion(q);
        Self::new(
            float4::new(m.c0.x, m.c0.y, m.c0.z, 0.0),
            float4::new(m.c1.x, m.c1.y, m.c1.z, 0.0),
            float4::new(m.c2.x, m.c2.y, m.c2.z, 0.0),
            float4::new(0.0, 0.0, 0.0, 1.0),
        )
    }

    /// `float4x4.TRS(translation, rotation, scale)` — the original builds the columns directly
    /// rather than multiplying three matrices, so the rounding order is its own.
    #[inline]
    pub fn trs(pos: float3, rot: quaternion, scl: float3) -> Self {
        let r = float3x3::from_quaternion(rot);
        Self::new(
            float4::new(r.c0.x * scl.x, r.c0.y * scl.x, r.c0.z * scl.x, 0.0),
            float4::new(r.c1.x * scl.y, r.c1.y * scl.y, r.c1.z * scl.y, 0.0),
            float4::new(r.c2.x * scl.z, r.c2.y * scl.z, r.c2.z * scl.z, 0.0),
            float4::new(pos.x, pos.y, pos.z, 1.0),
        )
    }

    /// The translation column, as `Unity.Transforms` stores it.
    #[inline]
    pub fn position(&self) -> float3 {
        float3::new(self.c3.x, self.c3.y, self.c3.z)
    }

    /// `float4x4.Ortho(width, height, near, far)` — centered orthographic, the original's form.
    /// Unity's clip convention here maps view-space depth to [-1, 1]; the renderer converts to the
    /// API depth range, exactly as the original's HDRP layer does.
    #[inline]
    pub fn ortho(width: f32, height: f32, near: f32, far: f32) -> Self {
        let rcpdz = 1.0 / (near - far);
        Self::from_elements(
            2.0 / width,
            0.0,
            0.0,
            0.0,
            0.0,
            2.0 / height,
            0.0,
            0.0,
            0.0,
            0.0,
            rcpdz,
            2.0 * far * near * rcpdz,
            0.0,
            0.0,
            0.0,
            1.0,
        )
    }

    /// `float4x4.PerspectiveFov(verticalFov, aspect, near, far)` — `verticalFov` is the full
    /// vertical field of view in radians. Term-for-term the original's construction.
    #[inline]
    pub fn perspective_fov(vertical_fov: f32, aspect: f32, near: f32, far: f32) -> Self {
        let cotangent = 1.0 / (vertical_fov * 0.5).tan();
        let rcpdz = 1.0 / (near - far);
        Self::from_elements(
            cotangent / aspect,
            0.0,
            0.0,
            0.0,
            0.0,
            cotangent,
            0.0,
            0.0,
            0.0,
            0.0,
            (far + near) * rcpdz,
            2.0 * near * far * rcpdz,
            0.0,
            0.0,
            -1.0,
            0.0,
        )
    }

    /// `float4x4.PerspectiveOffCenter` — the form the original's shadow and UI passes use.
    #[inline]
    pub fn perspective_off_center(l: f32, r: f32, b: f32, t: f32, near: f32, far: f32) -> Self {
        let rcpdz = 1.0 / (near - far);
        let rcp_width = 1.0 / (r - l);
        let rcp_height = 1.0 / (t - b);
        Self::from_elements(
            2.0 * near * rcp_width,
            0.0,
            (l + r) * rcp_width,
            0.0,
            0.0,
            2.0 * near * rcp_height,
            (b + t) * rcp_height,
            0.0,
            0.0,
            0.0,
            (far + near) * rcpdz,
            2.0 * near * far * rcpdz,
            0.0,
            0.0,
            -1.0,
            0.0,
        )
    }

    /// `float3x3.LookRotation(forward, up)` — the original's view basis, kept as its own function
    /// because `ProjectM` calls it directly for cameras and turrets.
    #[inline]
    pub fn look_rotation(forward: float3, up: float3) -> float3x3 {
        let n = |v: float3| {
            let l = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
            if l > 0.0 {
                v / l
            } else {
                v
            }
        };
        let t = n(up.cross(forward));
        float3x3::new(t, forward.cross(t), forward)
    }

    /// `float4x4.LookAt(eye, target, up)` — the original keeps the eye in the fourth column rather
    /// than negating the basis dot products; it is a world->view transform composed differently.
    #[inline]
    pub fn look_at(eye: float3, target: float3, up: float3) -> Self {
        let n = |v: float3| {
            let l = (v.x * v.x + v.y * v.y + v.z * v.z).sqrt();
            if l > 0.0 {
                v / l
            } else {
                v
            }
        };
        let rot = Self::look_rotation(n(target - eye), up);
        Self::new(
            float4::new(rot.c0.x, rot.c0.y, rot.c0.z, 0.0),
            float4::new(rot.c1.x, rot.c1.y, rot.c1.z, 0.0),
            float4::new(rot.c2.x, rot.c2.y, rot.c2.z, 0.0),
            float4::new(eye.x, eye.y, eye.z, 1.0),
        )
    }

    /// Determinant, expanded along the first row exactly as `float4x4.gen.cs` does, so the float
    /// rounding order matches the original.
    #[inline]
    pub fn determinant(&self) -> f32 {
        let c0 = self.c0;
        let c1 = self.c1;
        let c2 = self.c2;
        let c3 = self.c3;
        let m00 = c1.y * (c2.z * c3.w - c2.w * c3.z) - c2.y * (c1.z * c3.w - c1.w * c3.z)
            + c3.y * (c1.z * c2.w - c1.w * c2.z);
        let m01 = c0.y * (c2.z * c3.w - c2.w * c3.z) - c2.y * (c0.z * c3.w - c0.w * c3.z)
            + c3.y * (c0.z * c2.w - c0.w * c2.z);
        let m02 = c0.y * (c1.z * c3.w - c1.w * c3.z) - c1.y * (c0.z * c3.w - c0.w * c3.z)
            + c3.y * (c0.z * c1.w - c0.w * c1.z);
        let m03 = c0.y * (c1.z * c2.w - c1.w * c2.z) - c1.y * (c0.z * c2.w - c0.w * c2.z)
            + c2.y * (c0.z * c1.w - c0.w * c1.z);
        c0.x * m00 - c1.x * m01 + c2.x * m02 - c3.x * m03
    }

    /// Cofactor (adjugate) inverse: entry `(i, j)` of the result is `cofactor(j, i) / determinant`.
    /// The original computes the same thing through a SIMD shuffle network, which changes only the
    /// float rounding order; that is recorded in `docs/PORT.md`. The original has no singular guard
    /// and returns infinities, so the port's zero-return guard is a deliberate deviation.
    #[inline]
    pub fn inverse(&self) -> Self {
        let det = self.determinant();
        if det == 0.0 {
            return Self::default();
        }
        let inv = 1.0 / det;
        let cols: [[f32; 4]; 4] = [
            [self.c0.x, self.c0.y, self.c0.z, self.c0.w],
            [self.c1.x, self.c1.y, self.c1.z, self.c1.w],
            [self.c2.x, self.c2.y, self.c2.z, self.c2.w],
            [self.c3.x, self.c3.y, self.c3.z, self.c3.w],
        ];
        let cofactor = |i: usize, j: usize| -> f32 {
            // minor of the 3x3 left after removing row i and column j
            let mut pick: [[f32; 4]; 3] = [[0.0; 4]; 3];
            let mut k = 0;
            for (c, col) in cols.iter().enumerate() {
                if c != j {
                    pick[k] = *col;
                    k += 1;
                }
            }
            let mut rows = [0usize; 3];
            let mut k = 0;
            for r in 0..4 {
                if r != i {
                    rows[k] = r;
                    k += 1;
                }
            }
            let sign = if (i + j).is_multiple_of(2) { 1.0 } else { -1.0 };
            sign * det3(pick[0], pick[1], pick[2], rows[0], rows[1], rows[2])
        };
        // Column j of the adjugate holds the cofactors of *row* j.
        let col = |j: usize| {
            float4::new(
                cofactor(j, 0),
                cofactor(j, 1),
                cofactor(j, 2),
                cofactor(j, 3),
            ) * inv
        };
        Self::new(col(0), col(1), col(2), col(3))
    }

    /// The upper-left 3x3 rotation/scale part.
    #[inline]
    pub fn to_float3x3(&self) -> float3x3 {
        float3x3::new(
            float3::new(self.c0.x, self.c0.y, self.c0.z),
            float3::new(self.c1.x, self.c1.y, self.c1.z),
            float3::new(self.c2.x, self.c2.y, self.c2.z),
        )
    }
}

impl std::ops::Mul for float3x3 {
    type Output = float3x3;
    #[inline]
    fn mul(self, r: Self) -> Self {
        Self::mul_matrix(self, r)
    }
}

impl std::ops::Mul<float3> for float3x3 {
    type Output = float3;
    /// Columns weighted by the vector — the original's `mul`.
    #[inline]
    fn mul(self, r: float3) -> float3 {
        self.c0 * r.x + self.c1 * r.y + self.c2 * r.z
    }
}

impl std::ops::Mul for float4x4 {
    type Output = float4x4;
    #[inline]
    fn mul(self, r: Self) -> Self {
        Self::mul_matrix(self, r)
    }
}

impl std::ops::Mul<float4> for float4x4 {
    type Output = float4;
    #[inline]
    fn mul(self, r: float4) -> float4 {
        self.c0 * r.x + self.c1 * r.y + self.c2 * r.z + self.c3 * r.w
    }
}

/// `math.transpose` free-function spellings the systems call.
#[inline]
pub fn transpose3(m: float3x3) -> float3x3 {
    m.transpose()
}
#[inline]
pub fn transpose4(m: float4x4) -> float4x4 {
    m.transpose()
}

// `ToString` becomes `Display`. These two shapes are hand-written here rather than generated, so
// their format strings are copied from `evidence/math_tostring.tsv` by hand: the name, then the
// elements **row by row** with an `f` after each, and two spaces between rows. The fields are
// columns, hence the transposed argument order.

/// `float3x3.ToString()` — literal at VA 0xd9eb248.
impl std::fmt::Display for float3x3 {
    fn fmt(&self, w: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            w,
            "float3x3({}f, {}f, {}f,  {}f, {}f, {}f,  {}f, {}f, {}f)",
            self.c0.x,
            self.c1.x,
            self.c2.x,
            self.c0.y,
            self.c1.y,
            self.c2.y,
            self.c0.z,
            self.c1.z,
            self.c2.z,
        )
    }
}

/// `float4x4.ToString()` — literal at VA 0xd9f5ce8.
impl std::fmt::Display for float4x4 {
    fn fmt(&self, w: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            w,
            "float4x4({}f, {}f, {}f, {}f,  {}f, {}f, {}f, {}f,  {}f, {}f, {}f, {}f,  {}f, {}f, {}f, {}f)",
            self.c0.x,
            self.c1.x,
            self.c2.x,
            self.c3.x,
            self.c0.y,
            self.c1.y,
            self.c2.y,
            self.c3.y,
            self.c0.z,
            self.c1.z,
            self.c2.z,
            self.c3.z,
            self.c0.w,
            self.c1.w,
            self.c2.w,
            self.c3.w,
        )
    }
}
