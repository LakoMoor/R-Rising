//! Scalar half of `Unity.Mathematics.math` (`dump.cs` gives the class 1908 methods; this port has
//! the ones the game's systems call, listed in `docs/PORT.md`).
//!
//! Precision note: the original's transcendentals go through its own implementations so a client and
//! a server on different CPUs agree. The port uses Rust's `f32` intrinsics, which lower to the
//! platform libm. Where a value is networked or golden-tested this is a known deviation and is
//! recorded in `docs/PORT.md`; it is not silently widened.

/// `math.abs`
#[inline]
pub fn abs(x: f32) -> f32 {
    x.abs()
}
#[inline]
pub fn absi(x: i32) -> i32 {
    x.abs()
}
/// `math.min(float x, float y)` — the original is `IsNaN(y) || x < y ? x : y`: a NaN `y` returns
/// `x` and a NaN `x` returns `y`. Rust's `f32::min` picks the other way round, so it is not used.
#[inline]
pub fn min(x: f32, y: f32) -> f32 {
    if y.is_nan() || x < y {
        x
    } else {
        y
    }
}
/// `math.max(float x, float y)` — `IsNaN(y) || x > y ? x : y`.
#[inline]
pub fn max(x: f32, y: f32) -> f32 {
    if y.is_nan() || x > y {
        x
    } else {
        y
    }
}
#[inline]
pub fn mini(a: i32, b: i32) -> i32 {
    if a < b {
        a
    } else {
        b
    }
}
#[inline]
pub fn maxi(a: i32, b: i32) -> i32 {
    if a > b {
        a
    } else {
        b
    }
}
/// `math.clamp(float value, float lower, float upper)` — the original nests it as
/// `max(lower, min(upper, value))`, which is what decides a NaN input.
#[inline]
pub fn clamp(value: f32, lower: f32, upper: f32) -> f32 {
    max(lower, min(upper, value))
}
#[inline]
pub fn clampi(x: i32, a: i32, b: i32) -> i32 {
    mini(maxi(x, a), b)
}
/// `math.saturate`
#[inline]
pub fn saturate(x: f32) -> f32 {
    clamp(x, 0.0, 1.0)
}
/// `math.lerp(float a, float b, float t)` — the original's form is `a + (b - a) * t`.
#[inline]
pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
#[inline]
pub fn lerpi(a: i32, b: i32, t: f32) -> f32 {
    a as f32 + (b as f32 - a as f32) * t
}
/// `math.step(float edge, float x)`
#[inline]
pub fn step(edge: f32, x: f32) -> f32 {
    if x >= edge {
        1.0
    } else {
        0.0
    }
}
/// `math.smoothstep(float a, float b, float x)`
#[inline]
pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = saturate((x - a) / (b - a));
    t * t * (3.0 - 2.0 * t)
}
/// `math.sign`
#[inline]
pub fn sign(x: f32) -> f32 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        0.0
    }
}
#[inline]
pub fn signi(x: i32) -> i32 {
    x.signum()
}

#[inline]
pub fn sqrt(x: f32) -> f32 {
    x.sqrt()
}
/// `math.rsqrt` — the original computes `1 / sqrt(x)`.
#[inline]
pub fn rsqrt(x: f32) -> f32 {
    1.0 / x.sqrt()
}
#[inline]
pub fn rcp(x: f32) -> f32 {
    1.0 / x
}
#[inline]
pub fn pow(x: f32, y: f32) -> f32 {
    x.powf(y)
}
#[inline]
pub fn exp(x: f32) -> f32 {
    x.exp()
}
#[inline]
pub fn log(x: f32) -> f32 {
    x.ln()
}
#[inline]
pub fn log10(x: f32) -> f32 {
    x.log10()
}
#[inline]
pub fn log2(x: f32) -> f32 {
    x.log2()
}

#[inline]
pub fn sin(x: f32) -> f32 {
    x.sin()
}
#[inline]
pub fn cos(x: f32) -> f32 {
    x.cos()
}
#[inline]
pub fn tan(x: f32) -> f32 {
    x.tan()
}
#[inline]
pub fn asin(x: f32) -> f32 {
    x.asin()
}
#[inline]
pub fn acos(x: f32) -> f32 {
    x.acos()
}
#[inline]
pub fn atan(x: f32) -> f32 {
    x.atan()
}
#[inline]
pub fn atan2(y: f32, x: f32) -> f32 {
    y.atan2(x)
}

#[inline]
pub fn floor(x: f32) -> f32 {
    x.floor()
}
#[inline]
pub fn ceil(x: f32) -> f32 {
    x.ceil()
}
/// `math.round` — the original is `(float)System.Math.Round((float)x)`, i.e. round-half-to-even.
/// Rust spells that `round_ties_even`; round-half-away-from-zero is a different function.
#[inline]
pub fn round(x: f32) -> f32 {
    x.round_ties_even()
}
#[inline]
pub fn trunc(x: f32) -> f32 {
    x.trunc()
}
/// `math.frac` — the original is `x - floor(x)`, so `frac(-2.5)` is `0.5`. Rust's `fract()`
/// subtracts the truncation instead and would give `-0.5`.
#[inline]
pub fn frac(x: f32) -> f32 {
    x - x.floor()
}

/// `math.radians` / `math.degrees` — the original multiplies by the `float` forms of the double
/// constants `TORADIANS_DBL` and `TODEGREES_DBL`; the literals below are those `float` values, so
/// the rounding of the constant is part of the result.
#[inline]
pub fn radians(deg: f32) -> f32 {
    deg * 0.017453292519943295
}
#[inline]
pub fn degrees(rad: f32) -> f32 {
    rad * 57.29577951308232
}

#[inline]
pub fn isfinite(x: f32) -> bool {
    x.is_finite()
}
#[inline]
pub fn isnan(x: f32) -> bool {
    x.is_nan()
}
#[inline]
pub fn isinf(x: f32) -> bool {
    x.is_infinite()
}
/// `math.fmod` — Rust's `%` on floats already follows the same sign rules as the original's `x % y`.
#[inline]
pub fn fmod(x: f32, y: f32) -> f32 {
    x % y
}

/// The constants `math.PI`, `math.E`, `math.EPSILON`, `math.INFINITY`, `math.NAN` and the range
/// constants the systems read.
pub const PI: f32 = core::f32::consts::PI;
pub const PI_QUARTER: f32 = core::f32::consts::FRAC_PI_4;
pub const PI_HALF: f32 = core::f32::consts::FRAC_PI_2;
pub const PI_TIMES_TWO: f32 = 6.2831855f32;
pub const E: f32 = core::f32::consts::E;
pub const EPSILON: f32 = 1.401298e-45f32;
pub const MIN_VALUE: f32 = f32::MIN;
pub const MAX_VALUE: f32 = f32::MAX;
pub const INFINITY: f32 = f32::INFINITY;
pub const NAN: f32 = f32::NAN;
