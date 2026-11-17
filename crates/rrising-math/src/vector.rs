//! Vector types ported from `Unity.Mathematics`.
//!
//! Field order and widths match the IL2CPP layout exactly (`dump.cs` shows `public float x; // 0x0`,
//! `// 0x4`, `// 0x8`), so a vector is byte-compatible with what the original stores in ECS chunks.
//! Member names keep the original spelling so a reviewer can diff against `dump.cs`.
//!
//! The original is source-generated: `float3` alone declares 152 members (27 two-to-three-component
//! swizzles, 81 four-component swizzles, 12 swizzle setters, the indexer, the operators). This port
//! generates the same surface from the dump's own member list — see `build.rs` — and keeps the
//! `math.*` statics the game calls as free functions below.

// The generated surface deliberately mirrors the original `Unity.Mathematics` source (diffable against
// dump.cs): `x: x` field inits, literal constants such as LN_2, the 9-argument float3x3 constructor and
// the `mul` name. The lints are allowed on this module only so the generated semantics stay identical.
#[allow(
    clippy::redundant_field_names,
    clippy::excessive_precision,
    clippy::approx_constant,
    clippy::neg_multiply,
    clippy::too_many_arguments,
    clippy::should_implement_trait
)]
mod generated {
    // The generated `half2`/`half3`/`half4` name the scalar type in their fields and statics.
    #[allow(unused_imports)]
    use crate::half;
    include!(concat!(env!("OUT_DIR"), "/vectors.rs"));
}
pub use generated::*;

// `math` helpers the game calls on vectors. Names keep the original spelling where the original is a
// static (`math.lengthsq`, `math.normalize`); the arity suffix is the port's, because Rust has no
// overloading.

/// `math.length(float2)`
#[inline]
pub fn length2(v: float2) -> f32 {
    (v.dot(v)).sqrt()
}
/// `math.lengthsq(float2)`
#[inline]
pub fn lengthsq2(v: float2) -> f32 {
    v.dot(v)
}
/// `math.length(float3)`
#[inline]
pub fn length(v: float3) -> f32 {
    (v.dot(v)).sqrt()
}
/// `math.lengthsq(float3)`
#[inline]
pub fn lengthsq(v: float3) -> f32 {
    v.dot(v)
}
/// `math.distance(float3, float3)`
#[inline]
pub fn distance(a: float3, b: float3) -> f32 {
    length(b - a)
}
/// `math.normalize(float3)` — the original is `rsqrt(dot(x, x)) * x`: it multiplies by the
/// reciprocal of the length rather than dividing, and has no zero guard, so a zero vector scales by
/// `+inf` and comes out NaN. The port keeps both, because a caller that reads a NaN position is
/// reading the same defect the original reads.
#[inline]
pub fn normalize(v: float3) -> float3 {
    (1.0 / v.dot(v).sqrt()) * v
}
/// `math.cross(float2, float2)` returns the scalar z of the 3D cross.
#[inline]
pub fn cross2(a: float2, b: float2) -> f32 {
    a.x * b.y - a.y * b.x
}

/// `math.select(bool c, T a, T b)`
#[inline]
pub fn select3(c: bool3, a: float3, b: float3) -> float3 {
    float3::new(
        if c.x { a.x } else { b.x },
        if c.y { a.y } else { b.y },
        if c.z { a.z } else { b.z },
    )
}
#[inline]
pub fn select4(c: bool4, a: float4, b: float4) -> float4 {
    float4::new(
        if c.x { a.x } else { b.x },
        if c.y { a.y } else { b.y },
        if c.z { a.z } else { b.z },
        if c.w { a.w } else { b.w },
    )
}

#[inline]
pub fn less3(a: float3, b: float3) -> bool3 {
    bool3::new(a.x < b.x, a.y < b.y, a.z < b.z)
}
#[inline]
pub fn greater3(a: float3, b: float3) -> bool3 {
    bool3::new(a.x > b.x, a.y > b.y, a.z > b.z)
}
#[inline]
pub fn less_equal3(a: float3, b: float3) -> bool3 {
    bool3::new(a.x <= b.x, a.y <= b.y, a.z <= b.z)
}
#[inline]
pub fn greater_equal3(a: float3, b: float3) -> bool3 {
    bool3::new(a.x >= b.x, a.y >= b.y, a.z >= b.z)
}
#[inline]
pub fn min3(a: float3, b: float3) -> float3 {
    select3(less3(a, b), a, b)
}
#[inline]
pub fn max3(a: float3, b: float3) -> float3 {
    select3(greater3(a, b), a, b)
}
#[inline]
pub fn clamp3(v: float3, lo: float3, hi: float3) -> float3 {
    min3(max3(v, lo), hi)
}
/// `math.lerp(T a, T b, float t)` — the original's form is `a + (b - a) * t`.
#[inline]
pub fn lerp3(a: float3, b: float3, t: f32) -> float3 {
    a + (b - a) * t
}
#[inline]
pub fn lerp2(a: float2, b: float2, t: f32) -> float2 {
    a + (b - a) * t
}
#[inline]
pub fn lerp4(a: float4, b: float4, t: f32) -> float4 {
    a + (b - a) * t
}
