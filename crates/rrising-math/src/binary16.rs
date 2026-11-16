//! `Unity.Mathematics.half` — the IEEE binary16 scalar the original stores as a `ushort`.
//!
//! Transcribed from the shipped bodies, all from `evidence/decompiled/`:
//!
//! * `.ctor(float)` @ 0x93546a0 and `op_Explicit(float)` @ 0x929f960 store `math.f32tof16`
//!   @ 0x92b9520 into the 16-bit field; `.ctor(double)` @ 0x93546c0 narrows to `float` first.
//! * `op_Implicit(half) -> float` @ 0x612ac20 is `math.f16tof32` @ 0x612f520 inlined, and
//!   `op_Implicit(half) -> double` @ 0x93546e0 widens that same result.
//! * `op_Equality` @ 0x17d88d0 and `Equals(half)` @ 0x17d8950 compare the raw stored shorts, and
//!   `GetHashCode` @ 0x17d6a80 returns it.
//! * `get_MaxValue` @ 0x9354660 returns the `65504.0f` at 0x18cf29538 and `get_MinValue`
//!   @ 0x9354670 the `-65504.0f` at 0x18cf29b14; `get_MaxValueAsHalf` @ 0x9354680 and
//!   `get_MinValueAsHalf` @ 0x9354690 are the same `f32tof16` applied to those two constants,
//!   which is `0x7bff` and `0xfbff`.
//! * `ToString()` @ 0x93547e0 converts to `float` and formats it through .NET's number formatting.

use crate::mathfn::{f16tof32, f32tof16};

/// `Unity.Mathematics.half` — `public ushort value` at offset 0x0.
///
/// `PartialEq`, `Eq` and `Hash` are derived on the stored pattern because that is what the bodies
/// do: two `half`s are equal when their bit patterns match. So `+0.0` and `-0.0` are *not* equal
/// (unlike `float`'s `==`), and a NaN is equal to itself.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct half {
    /// `value` — the raw binary16 bit pattern.
    pub value: u16,
}

impl half {
    /// `half.zero` — the one static field the shipped metadata keeps for this type.
    pub const zero: half = half { value: 0 };
    /// `half.MaxValue` — a `float`, as the property is: the `65504.0f` at 0x18cf29538.
    pub const max_value: f32 = 65504.0;
    /// `half.MinValue` — a `float`: the `-65504.0f` at 0x18cf29b14.
    pub const min_value: f32 = -65504.0;
    /// `half.MaxValueAsHalf` — `f32tof16(MaxValue)`.
    pub const max_value_as_half: half = half { value: 0x7bff };
    /// `half.MinValueAsHalf` — `f32tof16(MinValue)`.
    pub const min_value_as_half: half = half { value: 0xfbff };

    // The library's other statics. The shipped metadata keeps only `zero` on every vector type
    // (see `evidence/math_fields.tsv` and docs/PORT.md), because IL2CPP trimmed the rest; these
    // are the values the untrimmed library initialises them to, kept because the port needs them.
    /// `1.0` in binary16.
    pub const one: half = half { value: 0x3c00 };
    /// The smallest normal binary16, `2^-14` = `6.103515625e-05`.
    pub const eps: half = half { value: 0x0400 };
    /// `half.default_value`.
    pub const default_value: half = half::zero;

    /// `half(half x)` — the copy constructor the dump lists first.
    #[inline]
    pub const fn new(x: half) -> half {
        x
    }

    /// `half(float v)` — `.ctor(float)` @ 0x93546a0: `f32tof16(v)` truncated to the 16 stored
    /// bits. `f32tof16` is not saturating and can return 17 bits for a finite input above the
    /// binary16 range; the field assignment drops the extra bit, exactly as the body does.
    #[inline]
    pub fn from_f32(v: f32) -> half {
        half {
            value: f32tof16(v) as u16,
        }
    }

    /// `half(double v)` — `.ctor(double)` @ 0x93546c0: narrows to `float`, then the same.
    #[inline]
    pub fn from_f64(v: f64) -> half {
        half {
            value: f32tof16(v as f32) as u16,
        }
    }

    /// The stored pattern, without converting.
    #[inline]
    pub const fn to_bits(self) -> u16 {
        self.value
    }

    /// A `half` from a stored pattern, without converting.
    #[inline]
    pub const fn from_bits(v: u16) -> half {
        half { value: v }
    }

    /// `half.op_Implicit(half) -> float` — `op_Implicit_9` @ 0x612ac20.
    #[inline]
    pub fn to_f32(self) -> f32 {
        f16tof32(self.value as i32)
    }

    /// `half.op_Implicit(half) -> double` — `op_Implicit_10` @ 0x93546e0.
    #[inline]
    pub fn to_f64(self) -> f64 {
        self.to_f32() as f64
    }
}

/// `half.op_Explicit(float v)` @ 0x929f960 and `half.op_Explicit(double v)` @ 0x929f970. Rust has
/// one infallible conversion trait where C# has two spellings, so the narrowing forms become
/// `From` too; see docs/PORT.md.
impl From<f32> for half {
    #[inline]
    fn from(v: f32) -> half {
        half::from_f32(v)
    }
}

impl From<f64> for half {
    #[inline]
    fn from(v: f64) -> half {
        half::from_f64(v)
    }
}

/// `half.op_Implicit(half d)` @ 0x612ac20 and @ 0x93546e0.
impl From<half> for f32 {
    #[inline]
    fn from(v: half) -> f32 {
        v.to_f32()
    }
}

impl From<half> for f64 {
    #[inline]
    fn from(v: half) -> f64 {
        v.to_f64()
    }
}

/// `half.ToString()` @ 0x93547e0 — the value as a `float`, formatted. The original goes through
/// `System.NumberFormatter` with the current culture; Rust's `{}` on the same `float` is the
/// nearest portable counterpart, and it is what the generated vector `Display` impls call.
impl std::fmt::Display for half {
    fn fmt(&self, w: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(w, "{}", self.to_f32())
    }
}
