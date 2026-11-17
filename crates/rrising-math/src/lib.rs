//! `rrising-math` — a port of Unity's `Unity.Mathematics` assembly.
//!
//! Source of truth: the IL2CPP metadata dump of the shipped build
//! (`global-metadata.dat` v31, `dump.cs`), not a recollection of the Unity.Mathematics API. Field
//! offsets and names come from the dump; method bodies come from the decompiled `GameAssembly.dll`
//! where the algorithm is not obvious from the signature, and each such place carries a
//! `// src:` comment.
//!
//! Why this crate exists: `Unity.Mathematics` is managed code compiled into the game's own
//! `GameAssembly.dll`, so it is part of the program being ported, not part of the engine the port
//! may borrow. Everything the gameplay and ECS layers compute — positions, rotations, matrix
//! uploads, hash keys — goes through these functions, so their operation order is fixed here and
//! nowhere else.
//!
//! Naming note: the original's members are PascalCase (`float3.zero`, `quaternion.identity`,
//! `math.PI`). The port keeps them lower-case-cased as Rust requires but *not* renamed, and allows
//! the `non_upper_case_globals` lint so a reviewer can still match names one to one.
// This crate transcribes the original `Unity.Mathematics` bodies bit-for-bit: the 9-argument matrix
// constructor, `a * b * -1.0` evaluation order and the static `mul(a, b)` name are the original shape,
// so these style lints are allowed crate-wide rather than rewriting numerics.
#![allow(
    clippy::too_many_arguments,
    clippy::neg_multiply,
    clippy::should_implement_trait,
    clippy::excessive_precision,
    clippy::approx_constant
)]
#![allow(non_upper_case_globals, non_camel_case_types, dead_code)]

mod binary16;
mod euler;
mod mathfn;
mod matrix;
mod quat;
mod rigid;
mod scalar;
mod vector;
pub use binary16::half;
pub use mathfn::*;
pub use matrix::{float3x3, float4x4, transpose3, transpose4};
pub use quat::quaternion;
pub use rigid::{
    hash_rigid, hashwide_rigid, rigid_transform, rigid_transform_m3, rigid_transform_m4,
    RigidTransform,
};
pub use scalar::*;
pub use vector::{
    bool2, bool3, bool4, clamp3, cross2, distance, double2, double3, double4, float2, float3,
    float4, greater3, greater_equal3, half2, half3, half4, int2, int3, int4, length, length2,
    lengthsq, lengthsq2, lerp2, lerp3, lerp4, less3, less_equal3, max3, min3, normalize, select3,
    select4, uint2, uint3, uint4,
};

/// The generated matrix shapes. `float3x3` and `float4x4` are the two the port hand-writes in
/// `matrix.rs` — they carry the projection and view constructors — so those names come from there
/// instead, and the generated copies of them stay crate-internal.
pub use vector::{
    bool2x2, bool2x3, bool2x4, bool3x2, bool3x3, bool3x4, bool4x2, bool4x3, bool4x4, double2x2,
    double2x3, double2x4, double3x2, double3x3, double3x4, double4x2, double4x3, double4x4,
    float2x2, float2x3, float2x4, float3x2, float3x4, float4x2, float4x3, int2x2, int2x3, int2x4,
    int3x2, int3x3, int3x4, int4x2, int4x3, int4x4, uint2x2, uint2x3, uint2x4, uint3x2, uint3x3,
    uint3x4, uint4x2, uint4x3, uint4x4,
};

/// `math` as a namespace, so ported systems can write `math::clamp(...)` the way the C# does.
pub mod math {
    pub use crate::mathfn::*;
    pub use crate::quat::quaternion;
    pub use crate::rigid::{
        hash_rigid, hashwide_rigid, rigid_transform, rigid_transform_m3, rigid_transform_m4,
        RigidTransform,
    };
    pub use crate::scalar::*;
    pub use crate::vector::{
        clamp3, cross2, distance, greater3, greater_equal3, length, length2, lengthsq, lengthsq2,
        lerp2, lerp3, lerp4, less3, less_equal3, max3, min3, normalize, select3, select4,
    };
    pub use crate::{
        bool2, bool3, bool4, double2, double3, double4, float2, float3, float4, half, half2, half3,
        half4, int2, int3, int4, uint2, uint3, uint4,
    };

    /// `math.RotationOrder` — the shipped enum, from `dump.cs`: `XYZ = 0, XZY = 1, YXZ = 2,
    /// YZX = 3, ZXY = 4, ZYX = 5, Default = 4`. This build has no `None` member, and its default
    /// is `ZXY`, which is why `quaternion.Euler`'s recorded default argument is `4`.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    #[repr(i32)]
    pub enum RotationOrder {
        XYZ = 0,
        XZY = 1,
        YXZ = 2,
        YZX = 3,
        ZXY = 4,
        ZYX = 5,
    }

    /// `math.RotationOrder.Default` — `ZXY`, the value the dump records for it.
    pub const DEFAULT_ROTATION_ORDER: RotationOrder = RotationOrder::ZXY;
}

pub use crate::math::RotationOrder;
