//! Pins the `half` family to the shipped bodies.
//!
//! The scalar conversions themselves are covered by `math_statics.rs` (`f16tof32`/`f32tof16`);
//! what this file pins is how `Unity.Mathematics.half` and the generated `halfN` vectors use them:
//! the 16-bit truncation in the constructor, the bitwise equality, the four static getters, and the
//! conversion set the dump actually declares (which is *not* every family pair).

use rrising_math::{bool2, double4, float2, float3, float4, half, half2, half3, half4};

/// Builds a `halfN` from floats, the way the original's `new half4(1.5f, ...)` would.
fn h2(a: f32, b: f32) -> half2 {
    half2::new(half::from_f32(a), half::from_f32(b))
}
fn h3(a: f32, b: f32, c: f32) -> half3 {
    half3::new(half::from_f32(a), half::from_f32(b), half::from_f32(c))
}
fn h4(a: f32, b: f32, c: f32, d: f32) -> half4 {
    half4::new(
        half::from_f32(a),
        half::from_f32(b),
        half::from_f32(c),
        half::from_f32(d),
    )
}

#[test]
fn half_round_trips_every_finite_pattern_through_the_type() {
    // `.ctor(float)` @ 0x93546a0 stores `f32tof16(v)`; `op_Implicit` @ 0x612ac20 reads it back.
    // Every pattern with a non-0x1f exponent field is a finite value that survives the trip.
    let mut checked = 0u32;
    for bits in 0u16..=0xffff {
        let h = half::from_bits(bits);
        if (bits >> 10) & 0x1f == 0x1f && bits & 0x3ff != 0 {
            continue; // NaN: not equal to itself under the original's own comparison
        }
        assert_eq!(
            half::from_f32(h.to_f32()).to_bits(),
            bits,
            "pattern {bits:#06x}"
        );
        checked += 1;
    }
    // 65536 patterns minus the 2046 NaN ones (exponent all ones, mantissa non-zero, both signs);
    // the two Inf patterns are included and do survive the trip.
    assert_eq!(checked, 65_536 - 2 * 1023, "every finite binary16 pattern");
}

#[test]
fn the_four_static_getters_return_the_bodies_values() {
    // get_MaxValue @ 0x9354660 (`_DAT_18cf29538`) and get_MinValue @ 0x9354670 (`_DAT_18cf29b14`)
    // are floats; the `...AsHalf` pair @ 0x9354680/0x9354690 are `f32tof16` of the same constants.
    assert_eq!(half::max_value, 65504.0);
    assert_eq!(half::min_value, -65504.0);
    assert_eq!(half::max_value_as_half.to_bits(), 0x7bff);
    assert_eq!(half::min_value_as_half.to_bits(), 0xfbff);
    assert_eq!(half::max_value_as_half.to_f32(), half::max_value);
    assert_eq!(half::min_value_as_half.to_f32(), half::min_value);
}

#[test]
fn equality_is_the_stored_pattern_not_the_value() {
    // op_Equality @ 0x17d88d0 is `param_1 == param_2` on the shorts, and GetHashCode @ 0x17d6a80
    // returns the short. So +0.0 and -0.0 are different halves even though they compare equal as
    // floats, and a NaN is equal to itself.
    let pos = half::from_f32(0.0);
    let neg = half::from_f32(-0.0);
    assert_eq!(pos.to_bits(), 0x0000);
    assert_eq!(neg.to_bits(), 0x8000);
    assert!(pos.to_f32() == neg.to_f32());
    assert_ne!(pos, neg);
    let nan = half::from_bits(0x7e00);
    assert_eq!(nan, nan);
    assert_ne!(nan, half::from_bits(0x7dff));
}

#[test]
fn the_constructor_truncates_to_the_stored_16_bits() {
    // `f32tof16` is not saturating: a finite float above the binary16 range returns 17 bits, and
    // the field assignment in `.ctor(float)` keeps only the low 16. 1e30 is the clearest case.
    assert_eq!(half::from_f32(70000.0).to_bits(), 0x7c46);
    assert_eq!(half::from_f32(1e30).to_bits(), 0xca50);
    assert_eq!(half::from_f32(1e30).to_f32(), -12.625);
    // Inf and NaN do come through as the binary16 patterns, because the body's signed test catches
    // them before the round-off.
    assert_eq!(half::from_f32(f32::INFINITY).to_bits(), 0x7c00);
    assert_eq!(half::from_f32(f32::NEG_INFINITY).to_bits(), 0xfc00);
    assert_eq!(half::from_f32(f32::NAN).to_bits(), 0x7e00);
    // `.ctor(double)` @ 0x93546c0 narrows to float first, so a double outside float's range is
    // already Inf by the time the conversion runs.
    assert_eq!(half::from_f64(1e300).to_bits(), 0x7c00);
}

#[test]
fn vectors_store_halves_and_convert_to_the_float_forms_the_dump_declares() {
    let v = half3::new(
        half::from_f32(1.5),
        half::from_f32(-2.25),
        half::from_f32(3.75),
    );
    // `float3 op_Implicit(half3)` — widening, so it is exact for these values.
    let f = float3::from(v);
    assert_eq!(f, float3::new(1.5, -2.25, 3.75));
    // `half3 op_Explicit(float3)` — the same values back.
    assert_eq!(half3::from(f), v);
    // `half3 op_Implicit(half v)` splats the scalar.
    let s = half3::from(half::from_f32(0.5));
    assert_eq!(
        s,
        half3::new(
            half::from_f32(0.5),
            half::from_f32(0.5),
            half::from_f32(0.5)
        )
    );
    // and the float splat `half3 op_Explicit(float v)`.
    assert_eq!(half3::from(0.5f32), s);
}

#[test]
fn only_the_conversions_the_dump_declares_exist() {
    // The ledger lists `half4` conversions from `half`, `float`, `double`, `float4` and `double4`
    // and nothing else — no `int4`, no `uint4`, no `bool4`.
    let h = half4::from(1.0f64);
    assert_eq!(float4::from(h), float4::splat(1.0));
    assert_eq!(half4::from(1.0f32), h);
    assert_eq!(half4::from(1.0f64), h);
    // The vector forms the dump declares: `half4 op_Explicit(float4)` and `op_Explicit(double4)`.
    assert_eq!(half4::from_float(float4::splat(1.0)), h);
    assert_eq!(half4::from_double(double4::splat(1.0)), h);
    // `half4::from_int` / `from_bool` are absent: the dump declares no such operator, and the
    // generator's cross-check fails the build if one is emitted anyway.
}
#[test]
fn swizzles_and_the_indexer_use_the_declaration_order() {
    let v = h4(1.0, 2.0, 3.0, 4.0);
    assert_eq!(v.xyz(), h3(1.0, 2.0, 3.0));
    assert_eq!(v.wxyz(), h4(4.0, 1.0, 2.0, 3.0));
    assert_eq!(v.item(2), half::from_f32(3.0));
    assert_eq!(v[0], half::from_f32(1.0));
    let mut m = v;
    m.set_xy(h2(9.0, 8.0));
    assert_eq!(m.xy(), h2(9.0, 8.0));
    assert_eq!(m.zw(), v.zw());
}

#[test]
fn component_wise_equality_returns_booln_not_bool() {
    let a = h2(1.0, 2.0);
    let b = h2(1.0, 3.0);
    assert_eq!(a.is_equal(b), bool2::new(true, false));
    assert_eq!(a.is_not_equal(b), bool2::new(false, true));
    // Rust's `==` is the original's `Equals`, which is the same component-wise test folded to one
    // bool.
    assert_ne!(a, b);
    assert_eq!(a, h2(1.0, 2.0));
}

#[test]
fn display_prints_the_converted_float() {
    // ToString() @ 0x93547e0 formats `f16tof32(value)`; the vector form is the shipped literal
    // `half2({0}, {1})` — the type name, and no `f` suffix, which only the float families get.
    assert_eq!(half::from_f32(1.5).to_string(), "1.5");
    assert_eq!(h2(1.5, -2.5).to_string(), "half2(1.5, -2.5)");
}
#[test]
fn zero_is_the_only_static_the_shipped_metadata_keeps() {
    // evidence/math_fields.tsv lists exactly one static field per vector type. The port keeps the
    // library's other statics too (docs/PORT.md records why), but `zero` must be the pattern 0.
    assert_eq!(half::zero.to_bits(), 0);
    assert_eq!(half2::zero, half2::new(half::zero, half::zero));
    assert_eq!(half4::zero.x, half::zero);
    let _ = float2::one; // the untrimmed library's other statics are still available
}
