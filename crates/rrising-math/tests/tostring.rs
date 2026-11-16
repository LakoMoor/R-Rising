//! Pins for the `ToString` formats. Each string here is the literal the shipped build formats with,
//! copied from `evidence/math_tostring.tsv` (which `tools/emit_type_facts.py` extracts from
//! `stringliteral.json`); the generator emits them verbatim, so this test fails the moment the
//! generator's idea of the format drifts from the build's.

use rrising_math::{
    bool2x2, double2, float2, float3, float3x3, float4, float3x4, float4x3, float4x4, half, half2,
    int2, int4x4, quaternion, uint4,
};

#[test]
fn vectors_print_their_name_then_their_components() {
    assert_eq!(float2::new(1.5, -2.5).to_string(), "float2(1.5f, -2.5f)");
    assert_eq!(float3::new(1.5, -2.5, 3.0).to_string(), "float3(1.5f, -2.5f, 3f)");
    assert_eq!(
        float4::new(0.0, 1.0, -0.5, 2.0).to_string(),
        "float4(0f, 1f, -0.5f, 2f)"
    );
    // Only the `float` families get the `f`; the library's generator emits no suffix for the others.
    assert_eq!(int2::new(1, -2).to_string(), "int2(1, -2)");
    assert_eq!(double2::new(1.5, -2.5).to_string(), "double2(1.5, -2.5)");
    assert_eq!(
        half2::new(half::from_f32(1.5), half::from_f32(-2.5)).to_string(),
        "half2(1.5, -2.5)"
    );
    assert_eq!(
        uint4::new(1, 2, 3, 4).to_string(),
        "uint4(1, 2, 3, 4)"
    );
    assert_eq!(
        quaternion::new(1.0, 2.0, 3.0, 4.0).to_string(),
        "quaternion(1f, 2f, 3f, 4f)"
    );
}

#[test]
fn matrices_print_row_by_row_with_two_spaces_between_rows() {
    // The fields are columns, so printing rows means walking the columns inside.
    let m = float3x3::from_elements(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0);
    assert_eq!(
        m.to_string(),
        "float3x3(1f, 2f, 3f,  4f, 5f, 6f,  7f, 8f, 9f)"
    );
    assert_eq!(
        float4x4::identity.to_string(),
        "float4x4(1f, 0f, 0f, 0f,  0f, 1f, 0f, 0f,  0f, 0f, 1f, 0f,  0f, 0f, 0f, 1f)"
    );
    // `float3x4` is three rows of four, `float4x3` is four rows of three; the grouping follows the
    // row length, not the field length.
    let a = float3x4::new(
        rrising_math::float3::new(1.0, 2.0, 3.0),
        rrising_math::float3::new(4.0, 5.0, 6.0),
        rrising_math::float3::new(7.0, 8.0, 9.0),
        rrising_math::float3::new(10.0, 11.0, 12.0),
    );
    assert_eq!(
        a.to_string(),
        "float3x4(1f, 4f, 7f, 10f,  2f, 5f, 8f, 11f,  3f, 6f, 9f, 12f)"
    );
    let b = float4x3::new(
        float4::new(1.0, 2.0, 3.0, 4.0),
        float4::new(5.0, 6.0, 7.0, 8.0),
        float4::new(9.0, 10.0, 11.0, 12.0),
    );
    assert_eq!(
        b.to_string(),
        "float4x3(1f, 5f, 9f,  2f, 6f, 10f,  3f, 7f, 11f,  4f, 8f, 12f)"
    );
    assert_eq!(
        int4x4::new(
            rrising_math::int4::splat(1),
            rrising_math::int4::splat(2),
            rrising_math::int4::splat(3),
            rrising_math::int4::splat(4),
        )
        .to_string(),
        "int4x4(1, 2, 3, 4,  1, 2, 3, 4,  1, 2, 3, 4,  1, 2, 3, 4)"
    );
    assert_eq!(
        bool2x2::new(
            rrising_math::bool2::new(true, false),
            rrising_math::bool2::new(false, true),
        )
        .to_string(),
        "bool2x2(true, false,  false, true)"
    );
}
