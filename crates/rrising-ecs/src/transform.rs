//! `Unity.Transforms` composition, ported on plain `f32` arrays.
//!
//! Component values stay raw serialized floats (matrices column-major as `c0..c3`), so a caller
//! holding scene bytes needs no math types; the arithmetic goes through `rrising-math`.
//!
//! Provenance: the corpus under `evidence/decompiled` has no `Unity.Transforms` module, so the
//! system-level composition is not translated from a decompiled body. The leaf math it calls
//! (`float4x4.TRS`, `float3x3(quaternion)`, `mul(float4x4, float4x4)`, `mul(quaternion, quaternion)`)
//! is bit-identical to the original's, replayed under Rosetta (`tools/capture_math_matrix.py`). The formulas are the Unity Entities `LocalTransform` / `Translation` / `Rotation` /
//! `PostTransformMatrix` / `Parent` semantics, selected by *measuring bitwise agreement with the
//! `LocalToWorld` values the original stored in its scene files*
//! (`crates/rrising-content/tests/transform_golden.rs`, `docs/COMPATIBILITY.md`). Where a variant was
//! measured and rejected, the module says so rather than keeping both.
use rrising_math::{float3, float4, float4x4, quaternion};

/// A column-major 4x4 as serialized (`c0.xyzw, c1.xyzw, c2.xyzw, c3.xyzw`).
pub type Mat = [f32; 16];

/// `Unity.Transforms.LocalTransform` (Position @0, Scale @12, Rotation @16; 32 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LocalTransform {
    pub position: [f32; 3],
    pub scale: f32,
    pub rotation: [f32; 4],
}

pub fn to_mat(m: float4x4) -> Mat {
    [
        m.c0.x, m.c0.y, m.c0.z, m.c0.w, m.c1.x, m.c1.y, m.c1.z, m.c1.w, m.c2.x, m.c2.y, m.c2.z,
        m.c2.w, m.c3.x, m.c3.y, m.c3.z, m.c3.w,
    ]
}

pub fn from_mat(m: &Mat) -> float4x4 {
    float4x4::new(
        float4::new(m[0], m[1], m[2], m[3]),
        float4::new(m[4], m[5], m[6], m[7]),
        float4::new(m[8], m[9], m[10], m[11]),
        float4::new(m[12], m[13], m[14], m[15]),
    )
}

/// `math.mul(float4x4, float4x4)`.
pub fn mul(a: &Mat, b: &Mat) -> Mat {
    to_mat(float4x4::mul_matrix(from_mat(a), from_mat(b)))
}

/// `LocalTransform.ToMatrix()` = `float4x4.TRS(Position, Rotation, float3(Scale))`.
///
/// `rrising_math::float4x4::trs` is bit-identical to the original's `float4x4.TRS` (and its
/// `float3x3(quaternion)` constructor) replayed from the pinned binary on 2,048 inputs
/// (`crates/rrising-math/tests/oracle_matrix.rs`), including the `+0.0` the constructor writes where
/// a plain product form leaves `-0.0`. Against stored scene `LocalToWorld` this form matches
/// bit for bit for 58,021 of 58,597 (99.0%) root `LocalTransform+Translation+Rotation` entities;
/// the remainder is discussed in `docs/COMPATIBILITY.md`.
pub fn local_transform_matrix(lt: &LocalTransform) -> Mat {
    to_mat(float4x4::trs(
        float3::new(lt.position[0], lt.position[1], lt.position[2]),
        quaternion::new(
            lt.rotation[0],
            lt.rotation[1],
            lt.rotation[2],
            lt.rotation[3],
        ),
        float3::new(lt.scale, lt.scale, lt.scale),
    ))
}

/// Translation / Rotation / uniform scale: the original's `float4x4.TRS` (see
/// [`local_transform_matrix`]). Earlier revisions composed `Translate * (Rotate * Scale)` to
/// reproduce a stored `+0.0`; with the constructor's real operation order that workaround is
/// unnecessary and `TRS` matches the stored values better (58,021 vs 56,159 of 58,597 bit-identical on
/// those roots).
pub fn trs_matrix(t: [f32; 3], r: [f32; 4], s: f32) -> Mat {
    local_transform_matrix(&LocalTransform {
        position: t,
        scale: s,
        rotation: r,
    })
}

/// `PostTransformMatrix`: `mul(local, post)`. `mul` is bit-identical to the original's
/// `math.mul(float4x4, float4x4)` (replayed). Measured against stored matrices this order is the
/// right one (the reverse order is far worse) but only ~27% of entities are bit-exact and ~98.6%
/// are within 1e-5; stored matrices keep the local translation exactly where the product adds the
/// post matrix's tiny translation terms, and even an identity post does not reproduce the stored
/// rotation bits for ~59% of such entities. The original's combination is not recovered.
pub fn post_transform(local: &Mat, post: &Mat) -> Mat {
    mul(local, post)
}

/// Child world matrix: `mul(parent_world, local)`.
pub fn child_to_world(parent_world: &Mat, local: &Mat) -> Mat {
    mul(parent_world, local)
}

#[cfg(test)]
mod tests {
    use super::*;

    const I: Mat = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];

    #[test]
    fn identity_trs_has_positive_zeros() {
        // A rotation with negative-zero lanes must not leak `-0.0` into the product form.
        let m = trs_matrix([0.0; 3], [-0.0, 0.0, -0.0, 1.0], 1.0);
        assert!(m.iter().zip(&I).all(|(a, b)| a.to_bits() == b.to_bits()));
    }

    #[test]
    fn child_composes_translation_and_scale() {
        let parent = trs_matrix([1.0, 2.0, 3.0], [0.0, 0.0, 0.0, 1.0], 2.0);
        let child = trs_matrix([1.0, 0.0, 0.0], [0.0, 0.0, 0.0, 1.0], 1.0);
        let w = child_to_world(&parent, &child);
        assert_eq!(&w[12..15], &[3.0, 2.0, 3.0]);
        assert_eq!(w[0], 2.0);
        let lt = LocalTransform {
            position: [1.0, 2.0, 3.0],
            scale: 2.0,
            rotation: [0.0, 0.0, 0.0, 1.0],
        };
        assert_eq!(local_transform_matrix(&lt), parent);
    }
}
