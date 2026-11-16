//! `Unity.Mathematics.math` — the static functions the game calls as `math.foo(...)`.
//!
//! Every body here is transcribed from the decompiled `GameAssembly.dll` (`evidence/decompiled/
//! Unity.Mathematics.dll.c`); the RVA of the function it came from is in the comment above each
//! item, and the float constants were read out of the shipped `.rdata` (their addresses are quoted
//! where used). Where the original declares one function per type, Rust gets one function:
//! `all`/`any`/`csum`/`cmax`/`cmin` dispatch through a trait so the call site keeps the original
//! spelling, and the rest are given the concrete overload the gameplay layer uses.
//!
//! Not here yet, because the decompiled body is a SIMD unroll whose op order I am not willing to
//! guess: the numeric `hash`/`hashwide` forms for the vector and matrix types — only the `boolN`
//! forms, and the `RigidTransform` pair (straight-line multiplies, in `rigid.rs`), are transcribed
//! from their own bodies. `fastinverse` is transcribed for `float4x4` from the shuffle unroll at
//! 0x929eac0; the `float3x4`, `double3x4` and `double4x4` overloads are not. `docs/PORT.md`
//! records this.

use crate::{
    bool2, bool3, bool4, clamp, double2, double3, double4, float2, float3, float4, int2, int3, int4,
    max, maxi, min, mini, quaternion, uint2, uint3, uint4,
};
/// `math.ShuffleComponent` — the enum the `shuffle` overloads take (`dump.cs`). The values are the
/// component index into the concatenation `left ++ right`, which is what the decompiled
/// `select_shuffle_component` (RVA 0x928c370) switches on: 0–3 read `left`, 4–7 read `right`, and
/// anything else throws `ArgumentException`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum ShuffleComponent {
    LeftX = 0,
    LeftY = 1,
    LeftZ = 2,
    LeftW = 3,
    RightX = 4,
    RightY = 5,
    RightZ = 6,
    RightW = 7,
}

// ---------------------------------------------------------------------------
// math.all / math.any — "every component is non-zero" / "some component is non-zero"
// src: math.all_1225 @ 0x92b2c40 (`x != 0 ? y : false`), math.any_1210 @ 0x92b2a30
// (`x != 0 ? true : y`) — the bool2 forms. Every width reduces the same two tests; the numeric
// forms compare the component against zero, which is what their bodies show.
// ---------------------------------------------------------------------------

/// Component mask: `math.all`/`math.any` over the vector families.
pub trait Mask {
    fn all(self) -> bool;
    fn any(self) -> bool;
}

macro_rules! mask_bool {
    ($ty:ident, [$($c:ident),+]) => {
        impl Mask for $ty {
            #[inline]
            fn all(self) -> bool {
                true $(&& self.$c)+
            }
            #[inline]
            fn any(self) -> bool {
                false $(|| self.$c)+
            }
        }
    };
}

macro_rules! mask_num {
    ($ty:ident, $zero:tt, [$($c:ident),+]) => {
        impl Mask for $ty {
            #[inline]
            fn all(self) -> bool {
                true $(&& self.$c != $zero)+
            }
            #[inline]
            fn any(self) -> bool {
                false $(|| self.$c != $zero)+
            }
        }
    };
}

mask_bool!(bool2, [x, y]);
mask_bool!(bool3, [x, y, z]);
mask_bool!(bool4, [x, y, z, w]);
mask_num!(float2, 0.0f32, [x, y]);
mask_num!(float3, 0.0f32, [x, y, z]);
mask_num!(float4, 0.0f32, [x, y, z, w]);
mask_num!(double2, 0.0f64, [x, y]);
mask_num!(double3, 0.0f64, [x, y, z]);
mask_num!(double4, 0.0f64, [x, y, z, w]);
mask_num!(int2, 0i32, [x, y]);
mask_num!(int3, 0i32, [x, y, z]);
mask_num!(int4, 0i32, [x, y, z, w]);
mask_num!(uint2, 0u32, [x, y]);
mask_num!(uint3, 0u32, [x, y, z]);
mask_num!(uint4, 0u32, [x, y, z, w]);

/// `math.all(boolN x)` — every component set.
#[inline]
pub fn all<T: Mask>(x: T) -> bool {
    x.all()
}

/// `math.any(boolN x)` — some component set.
#[inline]
pub fn any<T: Mask>(x: T) -> bool {
    x.any()
}

/// `math.select(T a, T b, bool c)` — `c ? b : a`, the order the body at RVA 0x92b2e50 shows.
/// The component-masked forms are `select3`/`select4` in `vector.rs`.
#[inline]
pub fn select<T>(a: T, b: T, c: bool) -> T {
    if c {
        b
    } else {
        a
    }
}

// ---------------------------------------------------------------------------
// math.csum / cmax / cmin — reduce across components
// src: math.csum_1442 @ 0x92b9180, math.cmax_1430 @ 0x92b8ef0 (`x <= y ? y : x`), math.cmin_1418 @
// 0x92b8c60 (`y <= x ? y : x`) — the int2 forms of the same reduction.
//
// The 4-wide forms reduce as two 2-wide halves — `(x + y) + (z + w)`, `max(max(x, y), max(z, w))` —
// not as a left-to-right fold, and `csum(float4)`/`csum(double4)` group that way while
// `csum(int4)` does not. The grouping changes float rounding, so each width is written out.
// ---------------------------------------------------------------------------

/// Reduce across components, as `math.csum`/`cmax`/`cmin` do.
pub trait Reduce {
    type Scalar;
    fn csum(self) -> Self::Scalar;
    fn cmax(self) -> Self::Scalar;
    fn cmin(self) -> Self::Scalar;
}

/// `math.min(double, double)` / `math.max(double, double)` — the same NaN rule as the `float` pair.
#[inline]
fn mind(x: f64, y: f64) -> f64 {
    if y.is_nan() || x < y {
        x
    } else {
        y
    }
}
#[inline]
fn maxd(x: f64, y: f64) -> f64 {
    if y.is_nan() || x > y {
        x
    } else {
        y
    }
}

macro_rules! reduce_impl {
    ($ty:ident, $s:ty, $v:ident, $csum:expr, $cmax:expr, $cmin:expr) => {
        impl Reduce for $ty {
            type Scalar = $s;
            fn csum(self) -> $s {
                let $v = self;
                $csum
            }
            #[inline]
            fn cmax(self) -> $s {
                let $v = self;
                $cmax
            }
            #[inline]
            fn cmin(self) -> $s {
                let $v = self;
                $cmin
            }
        }
    };
}

reduce_impl!(float2, f32, v, v.x + v.y, max(v.x, v.y), min(v.x, v.y));
reduce_impl!(
    float3,
    f32,
    v,
    v.x + v.y + v.z,
    max(max(v.x, v.y), v.z),
    min(min(v.x, v.y), v.z)
);
reduce_impl!(
    float4,
    f32,
    v,
    (v.x + v.y) + (v.z + v.w),
    max(max(v.x, v.y), max(v.z, v.w)),
    min(min(v.x, v.y), min(v.z, v.w))
);
reduce_impl!(double2, f64, v, v.x + v.y, maxd(v.x, v.y), mind(v.x, v.y));
reduce_impl!(
    double3,
    f64,
    v,
    v.x + v.y + v.z,
    maxd(maxd(v.x, v.y), v.z),
    mind(mind(v.x, v.y), v.z)
);
reduce_impl!(
    double4,
    f64,
    v,
    (v.x + v.y) + (v.z + v.w),
    maxd(maxd(v.x, v.y), maxd(v.z, v.w)),
    mind(mind(v.x, v.y), mind(v.z, v.w))
);
reduce_impl!(int2, i32, v, v.x + v.y, maxi(v.x, v.y), mini(v.x, v.y));
reduce_impl!(
    int3,
    i32,
    v,
    v.x + v.y + v.z,
    maxi(maxi(v.x, v.y), v.z),
    mini(mini(v.x, v.y), v.z)
);
reduce_impl!(
    int4,
    i32,
    v,
    v.x + v.y + v.z + v.w,
    maxi(maxi(v.x, v.y), maxi(v.z, v.w)),
    mini(mini(v.x, v.y), mini(v.z, v.w))
);
reduce_impl!(
    uint2,
    u32,
    v,
    v.x + v.y,
    if v.x > v.y { v.x } else { v.y },
    if v.x < v.y { v.x } else { v.y }
);
reduce_impl!(
    uint3,
    u32,
    v,
    v.x + v.y + v.z,
    {
        let a = if v.x > v.y { v.x } else { v.y };
        if a > v.z { a } else { v.z }
    },
    {
        let a = if v.x < v.y { v.x } else { v.y };
        if a < v.z { a } else { v.z }
    }
);
reduce_impl!(
    uint4,
    u32,
    v,
    v.x + v.y + v.z + v.w,
    {
        let a = if v.x > v.y { v.x } else { v.y };
        let b = if v.z > v.w { v.z } else { v.w };
        if a > b { a } else { b }
    },
    {
        let a = if v.x < v.y { v.x } else { v.y };
        let b = if v.z < v.w { v.z } else { v.w };
        if a < b { a } else { b }
    }
);

/// `math.csum(T x)` — sum of the components.
#[inline]
pub fn csum<T: Reduce>(x: T) -> T::Scalar {
    x.csum()
}
/// `math.cmax(T x)` — largest component.
#[inline]
pub fn cmax<T: Reduce>(x: T) -> T::Scalar {
    x.cmax()
}
/// `math.cmin(T x)` — smallest component.
#[inline]
pub fn cmin<T: Reduce>(x: T) -> T::Scalar {
    x.cmin()
}

// ---------------------------------------------------------------------------
// Bit-reinterpreting casts
// src: math.asfloat_751 @ 0x92a4f00 (`return param_1;` — a pure reinterpret), math.asint_732 @
// 0x92a4e10. `asuint`/`asdouble`/`aslong`/`asulong` are the same reinterpret in the other
// direction; their bodies are not in the decompiled focus set, and a reinterpret has no order to
// get wrong, so they are written from the dump's signatures.
// ---------------------------------------------------------------------------

/// `math.asfloat(int x)`
#[inline]
pub fn asfloat(x: i32) -> f32 {
    f32::from_bits(x as u32)
}
/// `math.asint(float x)`
#[inline]
pub fn asint(x: f32) -> i32 {
    x.to_bits() as i32
}
/// `math.asuint(float x)`
#[inline]
pub fn asuint(x: f32) -> u32 {
    x.to_bits()
}
/// `math.asdouble(long x)`
#[inline]
pub fn asdouble(x: i64) -> f64 {
    f64::from_bits(x as u64)
}
/// `math.aslong(double x)`
#[inline]
pub fn aslong(x: f64) -> i64 {
    x.to_bits() as i64
}
/// `math.asulong(double x)`
#[inline]
pub fn asulong(x: f64) -> u64 {
    x.to_bits()
}

// ---------------------------------------------------------------------------
// Bit tricks
// src: math.countbits_1317 @ 0x92b5b80, math.lzcnt_1326 @ 0x92b61e0, math.tzcnt_1337 @ 0x92b6540,
// math.reversebits_1346 @ 0x92b68b0, math.rol_1357 @ 0x92b71c0, math.ror_1367 @ 0x92b75d0,
// math.ispow2_785 @ 0x92a58e0, math.ceilpow2_1376 @ 0x4740e90, math.floorlog2_1394 @ 0x8e97ce0,
// math.ceillog2_1386 @ 0x8e97d30, math.bitmask_758 @ 0x92a4f20, math.fold_to_uint_1480 @ 0x92baab0.
//
// The original's lzcnt/tzcnt/floorlog2 are the "add 2^52 to a double and read the exponent" trick
// (constant 4503599627370496.0 at 0x18cf293f0); Rust's intrinsics compute the same numbers, so the
// port uses them and says so rather than reproducing the float dance.
// ---------------------------------------------------------------------------

/// `math.countbits(int x)` — population count.
#[inline]
pub fn countbits(x: i32) -> i32 {
    (x as u32).count_ones() as i32
}
/// `math.lzcnt(int x)` — leading zeros; 32 for zero, which is what the body returns too.
#[inline]
pub fn lzcnt(x: i32) -> i32 {
    (x as u32).leading_zeros() as i32
}
/// `math.tzcnt(int x)` — trailing zeros; 32 for zero.
#[inline]
pub fn tzcnt(x: i32) -> i32 {
    (x as u32).trailing_zeros() as i32
}
/// `math.reversebits(int x)`
#[inline]
pub fn reversebits(x: i32) -> i32 {
    (x as u32).reverse_bits() as i32
}
/// `math.rol(int x, int n)` — the body masks the shift with `0x1f` before rotating.
#[inline]
pub fn rol(x: i32, n: i32) -> i32 {
    (x as u32).rotate_left((n & 0x1f) as u32) as i32
}
/// `math.ror(int x, int n)`
#[inline]
pub fn ror(x: i32, n: i32) -> i32 {
    (x as u32).rotate_right((n & 0x1f) as u32) as i32
}
/// `math.ispow2(int x)` — the body: `x >= 1 && (x & (x - 1)) == 0`.
#[inline]
pub fn ispow2(x: i32) -> bool {
    x >= 1 && (x as u32 & (x as u32).wrapping_sub(1)) == 0
}
/// `math.ceilpow2(int x)` — the body spreads the bits of `x - 1` rightwards and adds one, so 0 and
/// 1 both give 1 and 0 gives 0 (the `x - 1` wraps for that input).
#[inline]
pub fn ceilpow2(x: i32) -> i32 {
    let mut v = (x as u32).wrapping_sub(1);
    v = (v >> 1) | v;
    v = (v >> 2) | v;
    v = (v >> 4) | v;
    v = (v >> 8) | v;
    v = (v >> 16) | v;
    v.wrapping_add(1) as i32
}
/// `math.floorlog2(int x)` — the body returns -1 for zero, the exponent otherwise.
#[inline]
pub fn floorlog2(x: i32) -> i32 {
    if x == 0 {
        -1
    } else {
        31 - (x as u32).leading_zeros() as i32
    }
}
/// `math.ceillog2(int x)` — the body tests `x - 1U != 0`, so `ceillog2(0)` is `floorlog2(0xFFFFFFFF)
/// + 1` = 32, not 0; only `x == 1` returns 0.
#[inline]
pub fn ceillog2(x: i32) -> i32 {
    let m = (x as u32).wrapping_sub(1);
    if m == 0 {
        0
    } else {
        floorlog2(m as i32) + 1
    }
}
/// `math.bitmask(bool4 value)` — the body sets bit i when component i is set.
#[inline]
pub fn bitmask(x: bool4) -> i32 {
    let mut m = if x.x { 1 } else { 0 };
    if x.y {
        m |= 2;
    }
    if x.z {
        m |= 4;
    }
    if x.w {
        m |= 8;
    }
    m
}
/// `math.fold_to_uint(double x)` — the body XORs the two halves of the 64-bit pattern.
#[inline]
pub fn fold_to_uint(x: f64) -> u32 {
    let b = x.to_bits();
    ((b >> 32) as u32) ^ (b as u32)
}

/// `math.compress(float* output, int index, float4 val, bool4 mask)` — the body at RVA 0x19ad7c0
/// writes each component whose mask bit is set to `output[index]` and returns the advanced index.
/// The original takes a raw pointer; the port takes a slice and indexes it, which is the same
/// memory with a bounds check.
#[inline]
pub fn compress(output: &mut [f32], index: i32, val: float4, mask: bool4) -> i32 {
    let mut i = index;
    for (v, m) in [(val.x, mask.x), (val.y, mask.y), (val.z, mask.z), (val.w, mask.w)] {
        if m {
            output[i as usize] = v;
            i += 1;
        }
    }
    i
}

// ---------------------------------------------------------------------------
// hash / hashwide — only the bool forms are transcribed (their bodies are constant-select sums).
// src: math.hash_3 @ 0x928c140 and math.hashwide_4 @ 0x928c1b0 (bool2), math.hash_33 @ 0x928d0a0
// and math.hashwide_34 @ 0x928d0f0 (bool3), math.hash_67 @ 0x928e0d0 and math.hashwide_68 @
// 0x928e130 (bool4). `hash` is *not* `csum(hashwide(..))`: the two use different constants, so
// both are written out. The numeric forms are xxhash-shaped (`hash_with_unaligned_loads`, Primes
// 1–5) and are left for a pass over their own bodies.
// ---------------------------------------------------------------------------

/// `math.hashwide(bool2 v)` — the body picks one of two constants per component.
#[inline]
pub fn hashwide(v: bool2) -> uint2 {
    uint2 {
        x: if v.x { 0x6e2bf6a9 } else { 0xc9f27fcb },
        y: if v.y { 0x6d2523b1 } else { 0xcc74b3b7 },
    }
}

/// `math.hashwide(bool3 v)` — the same shape, three components.
#[inline]
pub fn hashwide3(v: bool3) -> uint3 {
    uint3 {
        x: if v.x { 0xae131389 } else { 0x5f17300d },
        y: if v.y { 0x5d1b165b } else { 0x670d6809 },
        z: if v.z { 0x87096cd7 } else { 0x7af32c49 },
    }
}

/// `math.hashwide(bool4 v)` — four components.
#[inline]
pub fn hashwide4(v: bool4) -> uint4 {
    uint4 {
        x: if v.x { 0xb40ec62d } else { 0xfe31134f },
        y: if v.y { 0x565ed63f } else { 0x712a34d7 },
        z: if v.z { 0x93c30c2b } else { 0x9d77a59b },
        w: if v.w { 0xdcaf0351 } else { 0x4942ca39 },
    }
}

/// `math.hash(bool2 v)` — the body's own selects, added `y + x`.
#[inline]
pub fn hash(v: bool2) -> u32 {
    let x: u32 = if v.x { 0xfab0f7dd } else { 0x90a285bb };
    let y: u32 = if v.y { 0x625c45bd } else { 0x5d19e1d5 };
    y.wrapping_add(x)
}

/// `math.hash(bool3 v)` — the body adds `z + y + x`.
#[inline]
pub fn hash3(v: bool3) -> u32 {
    let x: u32 = if v.x { 0xaf816615 } else { 0xa1e92d39 };
    let y: u32 = if v.y { 0x9af8d62d } else { 0x4583c801 };
    let z: u32 = if v.z { 0xe3600729 } else { 0x9536a0f5 };
    z.wrapping_add(y).wrapping_add(x)
}

/// `math.hash(bool4 v)` — the body adds `((w + y) + x) + z`, in that order.
#[inline]
pub fn hash4(v: bool4) -> u32 {
    let x: u32 = if v.x { 0xc53f4755 } else { 0x5fff6b19 };
    let y: u32 = if v.y { 0x5e6cbf3b } else { 0x6985c229 };
    let z: u32 = if v.z { 0xb546f2a5 } else { 0xe13bb0b3 };
    let w: u32 = if v.w { 0xbbcf63e7 } else { 0xc3e0a3b9 };
    w.wrapping_add(y).wrapping_add(x).wrapping_add(z)
}

// ---------------------------------------------------------------------------
// Vector geometry
// src: math.mad_859 @ 0x92a7560 (`z + x*y`), math.distancesq_1192 @ 0x92b1d70,
// math.faceforward_1302 @ 0x92b4df0, math.reflect_1278 @ 0x92b35a0, math.refract_1284 @ 0x92b3960,
// math.project_1290 @ 0x92b4170, math.projectsafe_1293 @ 0x92b4390, math.normalizesafe_1162 @
// 0x92b1510. Constants read from `.rdata`: 1.0f at 0x18cf28c64, 1.17549435e-38f (f32::MIN_POSITIVE)
// at 0x18cf28584, +inf at 0x18cf295c0, -0.0f (the sign-flip mask) at 0x18cf2aee0.
// ---------------------------------------------------------------------------

/// `math.mad(float x, float y, float z)` — `z + x*y`, in that order.
#[inline]
pub fn mad(x: f32, y: f32, z: f32) -> f32 {
    z + x * y
}
/// `math.mad(float3, float3, float3)` — the component-wise form of the same expression.
#[inline]
pub fn mad3(x: float3, y: float3, z: float3) -> float3 {
    float3 {
        x: z.x + x.x * y.x,
        y: z.y + x.y * y.y,
        z: z.z + x.z * y.z,
    }
}

/// `math.distancesq(float3 a, float3 b)` — the body squares `b - a` component-wise and sums.
#[inline]
pub fn distancesq(a: float3, b: float3) -> f32 {
    let d = b - a;
    d.x * d.x + d.y * d.y + d.z * d.z
}

/// `math.faceforward(float3 N, float3 I, float3 Nref)` — the body negates `N` (by XORing the sign
/// bit) when `dot(Nref, I) >= 0`, so a NaN dot product leaves `N` alone.
#[inline]
pub fn faceforward(n: float3, i: float3, nref: float3) -> float3 {
    if nref.x * i.x + nref.y * i.y + nref.z * i.z >= 0.0 {
        -n
    } else {
        n
    }
}

/// `math.reflect(float3 I, float3 N)` — the body: `I - (N + N) * dot(N, I)`.
#[inline]
pub fn reflect(i: float3, n: float3) -> float3 {
    let d = n.x * i.x + n.y * i.y + n.z * i.z;
    float3 {
        x: i.x - (n.x + n.x) * d,
        y: i.y - (n.y + n.y) * d,
        z: i.z - (n.z + n.z) * d,
    }
}

/// `math.refract(float3 I, float3 N, float eta)` — the body at RVA 0x92b3960:
/// `k = 1 - (1 - dot(N,I)^2) * eta^2`; if `k < 0` the result is zero, else
/// `eta * I - (sqrt(k) + dot(N,I) * eta) * N`.
#[inline]
pub fn refract(i: float3, n: float3, eta: f32) -> float3 {
    let d = n.x * i.x + n.y * i.y + n.z * i.z;
    let k = 1.0 - (1.0 - d * d) * eta * eta;
    if k < 0.0 {
        return float3::zero;
    }
    let f = k.sqrt() + d * eta;
    float3 {
        x: eta * i.x - f * n.x,
        y: eta * i.y - f * n.y,
        z: eta * i.z - f * n.z,
    }
}

/// `math.project(float3 a, float3 b)` — the body: `(dot(b,a) / dot(b,b)) * b`.
#[inline]
pub fn project(a: float3, b: float3) -> float3 {
    let s = (b.x * a.x + b.y * a.y + b.z * a.z) / (b.x * b.x + b.y * b.y + b.z * b.z);
    b * s
}

/// `math.projectsafe(float3 a, float3 b, float3 defaultValue)` — the body projects, then returns
/// `defaultValue` unless every component is finite (its guard compares `abs(x) < +inf`, the
/// constant at 0x18cf295c0, which a NaN fails).
#[inline]
pub fn projectsafe(a: float3, b: float3, default: float3) -> float3 {
    let r = project(a, b);
    if r.x.is_finite() && r.y.is_finite() && r.z.is_finite() {
        r
    } else {
        default
    }
}

/// `math.normalizesafe(float3 value, float3 defaultOutput)` — the body normalizes only when
/// `lengthsq > 1.17549435e-38` (the constant at 0x18cf28584), and scales by `1 / sqrt(len2)`.
#[inline]
pub fn normalizesafe(value: float3, default: float3) -> float3 {
    let l2 = value.x * value.x + value.y * value.y + value.z * value.z;
    if f32::MIN_POSITIVE < l2 {
        value * (1.0 / l2.sqrt())
    } else {
        default
    }
}

/// `math.up()` / `down()` / `forward()` / `back()` / `left()` / `right()` — the unit vectors, read
/// off the bodies at 0x92ba140 (`up`), 0x92ba150 (`down`), 0x92ba170 (`forward`), 0x92ba180
/// (`back`), 0x92ba190 (`left`) and 0x92ba1a0 (`right`); each stores `0x3f800000`/`0xbf800000` into
/// exactly one component. `tests/math_statics.rs` pins all six.
#[inline]
pub fn up() -> float3 {
    float3 { x: 0.0, y: 1.0, z: 0.0 }
}
/// `math.down()`
#[inline]
pub fn down() -> float3 {
    float3 { x: 0.0, y: -1.0, z: 0.0 }
}
/// `math.forward()`
#[inline]
pub fn forward() -> float3 {
    float3 { x: 0.0, y: 0.0, z: 1.0 }
}
/// `math.back()`
#[inline]
pub fn back() -> float3 {
    float3 { x: 0.0, y: 0.0, z: -1.0 }
}
/// `math.left()`
#[inline]
pub fn left() -> float3 {
    float3 { x: -1.0, y: 0.0, z: 0.0 }
}
/// `math.right()`
#[inline]
pub fn right() -> float3 {
    float3 { x: 1.0, y: 0.0, z: 0.0 }
}

// ---------------------------------------------------------------------------
// Interpolation and scalar transcendental helpers
// src: math.unlerp_843 @ 0x92a6a90, math.remap_851 @ 0x92a6e40, math.modf_1132 @ 0x92b0430,
// math.sincos_1308 @ 0x92b51f0, math.exp2_1084 @ 0x92aecb0, math.exp10_1092 @ 0x92af010.
// exp2/exp10 scale by ln2 (0.6931472f at 0x18cf28b3c) and ln10 (2.3025851f at 0x18cf28f64) *in
// float*, then call the double `exp` and truncate back — the bodies do exactly that, so the
// multiply is `f32` here too.
// ---------------------------------------------------------------------------

/// `math.unlerp(float a, float b, float x)` — `(x - a) / (b - a)`.
#[inline]
pub fn unlerp(a: f32, b: f32, x: f32) -> f32 {
    (x - a) / (b - a)
}

/// `math.remap(float a, float b, float c, float d, float x)` — the body maps `x` from `[a,b]` onto
/// `[c,d]`: `((x - a) / (b - a)) * (d - c) + c`.
#[inline]
pub fn remap(a: f32, b: f32, c: f32, d: f32, x: f32) -> f32 {
    ((x - a) / (b - a)) * (d - c) + c
}

/// `math.modf(float x, out float i)` — the body stores `trunc(x)` into the out parameter and
/// returns `x - trunc(x)`.
#[inline]
pub fn modf(x: f32, i: &mut f32) -> f32 {
    let t = x.trunc();
    *i = t;
    x - t
}

/// `math.sincos(float x, out float s, out float c)`
#[inline]
pub fn sincos(x: f32, s: &mut f32, c: &mut f32) {
    let (si, ci) = x.sin_cos();
    *s = si;
    *c = ci;
}

/// `math.exp2(float x)` — the body: `(float)exp((double)(x * 0.6931472f))`.
#[inline]
pub fn exp2(x: f32) -> f32 {
    ((x * 0.6931472f32) as f64).exp() as f32
}

/// `math.exp10(float x)` — the body: `(float)exp((double)(x * 2.3025851f))`.
#[inline]
pub fn exp10(x: f32) -> f32 {
    ((x * 2.3025851f32) as f64).exp() as f32
}

// ---------------------------------------------------------------------------
// Quaternion exp/log and the normalizing lerp
// src: math.unitexp_1698 @ 0x92cc850, math.unitlog_1700 @ 0x92cca20, math.nlerp_1705 @ 0x92ccdb0.
// unitexp: inv = 1/sqrt(dot(q.xyz,q.xyz)), len = 1/inv, result.xyz = inv*q.xyz*sin(len), w = cos(len).
// unitlog: w clamped into [-1, 1] (the -1.0f at 0x18cf29990), angle = acos(w), scale = angle/sqrt(1-w^2),
// result.xyz = q.xyz*scale, w = 0. nlerp: if dot(a,b) < 0 negate b, lerp, then normalize by 1/sqrt(len2).
// ---------------------------------------------------------------------------

/// `math.unitexp(quaternion exp)` — the exponential of a pure-rotation quaternion.
#[inline]
pub fn unitexp(q: quaternion) -> quaternion {
    let v = q.value.xyz();
    let l2 = v.x * v.x + v.y * v.y + v.z * v.z;
    let inv = 1.0 / l2.sqrt();
    let len = 1.0 / inv;
    let (s, c) = len.sin_cos();
    quaternion {
        value: float4 {
            x: inv * v.x * s,
            y: inv * v.y * s,
            z: inv * v.z * s,
            w: c,
        },
    }
}

/// `math.unitlog(quaternion q)` — the logarithm of a unit quaternion (`unitlog_1700 @ 0x92cca20`).
/// The body clamps `w` with two `is-inf-or-nan || out-of-range` tests; `math.clamp` reproduces that
/// exactly (+Inf and NaN both land on +1, -Inf on -1), so the port keeps the clamp.
#[inline]
pub fn unitlog(q: quaternion) -> quaternion {
    let w = clamp(q.value.w, -1.0, 1.0);
    let angle = w.acos();
    let scale = (1.0 / (1.0 - w * w).sqrt()) * angle;
    quaternion {
        value: float4 {
            x: scale * q.value.x,
            y: scale * q.value.y,
            z: scale * q.value.z,
            w: 0.0,
        },
    }
}

/// `math.nlerp(quaternion a, quaternion b, float t)` — the body flips `b` when the dot product is
/// negative, lerps as `(b - a) * t + a`, and normalizes the result by `1 / sqrt(len2)`.
#[inline]
pub fn nlerp(a: quaternion, b: quaternion, t: f32) -> quaternion {
    let mut b = b;
    if a.value.x * b.value.x + a.value.y * b.value.y + a.value.z * b.value.z + a.value.w * b.value.w
        < 0.0
    {
        b.value = -b.value;
    }
    let v = b.value - a.value;
    let r = a.value + v * t;
    let l2 = r.x * r.x + r.y * r.y + r.z * r.z + r.w * r.w;
    let s = 1.0 / l2.sqrt();
    quaternion { value: r * s }
}

/// `math.orthonormalize(float3x3)` — transcribed from the body at RVA 0x92bae80 and confirmed line
/// for line against the shipped Unity.Mathematics C# source (`math.orthonormalize`). It is *not*
/// the textbook three-vector Gram-Schmidt: the input's third column is ignored, the second column
/// is projected against the *unnormalized* first column (so the output is only orthonormal when the
/// input's first column is already unit length), the third output column is the cross product of
/// the two normalized ones, and a degenerate pair falls back to the canonical basis. The threshold
/// is the `1e-30f` at 0x18cf28590.
#[inline]
pub fn orthonormalize(i: crate::float3x3) -> crate::float3x3 {
    let x = i.c0;
    let y = i.c1;
    let d = y.x * x.x + y.y * x.y + y.z * x.z;
    let p0 = y.x - x.x * d;
    let p1 = y.y - x.y * d;
    let p2 = y.z - x.z * d;
    let lx = (x.x * x.x + x.y * x.y + x.z * x.z).sqrt();
    let ly = (p1 * p1 + p0 * p0 + p2 * p2).sqrt();
    let degenerate = lx <= 1e-30 || ly <= 1e-30;
    let mut c0 = float3::new(x.x / lx, x.y / lx, x.z / lx);
    let mut c1 = float3::new(p0 / ly, p1 / ly, p2 / ly);
    if degenerate {
        c0 = float3::new(1.0, 0.0, 0.0);
        c1 = float3::new(0.0, 1.0, 0.0);
    }
    crate::float3x3::new(c0, c1, c0.cross(c1))
}

// ---------------------------------------------------------------------------
// shuffle / select_shuffle_component / the SSE-style interleaves
// src: math.shuffle_5 @ 0x928c210, math.select_shuffle_component_9 @ 0x928c370,
// math.movehl_1478 @ 0x92ba870, math.movelh_1476 @ 0x92ba630, math.unpackhi_1474 @ 0x92ba3f0,
// math.unpacklo_1472 @ 0x92ba1b0. The four interleave bodies are built out of
// `select_shuffle_component` with these indices, read off their calls:
//   movelh  -> 0, 1, 4, 5      movehl  -> 2, 3, 6, 7
//   unpacklo-> 0, 4, 1, 5      unpackhi-> 2, 6, 3, 7
// ---------------------------------------------------------------------------

/// `math.select_shuffle_component(T4 a, T4 b, ShuffleComponent c)` — one component out of the
/// concatenation `a ++ b`. Out of range panics where the original throws `ArgumentException`.
#[inline]
pub fn select_shuffle_component(a: float4, b: float4, c: ShuffleComponent) -> f32 {
    let i = c as i32;
    match i {
        0 => a.x,
        1 => a.y,
        2 => a.z,
        3 => a.w,
        4 => b.x,
        5 => b.y,
        6 => b.z,
        7 => b.w,
        _ => panic!("math.ShuffleComponent out of range"),
    }
}

/// `math.shuffle(float4 left, float4 right, ShuffleComponent x, y, z, w)` — four picks.
#[inline]
pub fn shuffle(
    left: float4,
    right: float4,
    x: ShuffleComponent,
    y: ShuffleComponent,
    z: ShuffleComponent,
    w: ShuffleComponent,
) -> float4 {
    float4 {
        x: select_shuffle_component(left, right, x),
        y: select_shuffle_component(left, right, y),
        z: select_shuffle_component(left, right, z),
        w: select_shuffle_component(left, right, w),
    }
}

/// `math.movelh(float4 a, float4 b)` — the low halves of both, kept in blocks: `a0 a1 b0 b1`.
#[inline]
pub fn movelh(a: float4, b: float4) -> float4 {
    shuffle(
        a,
        b,
        ShuffleComponent::LeftX,
        ShuffleComponent::LeftY,
        ShuffleComponent::RightX,
        ShuffleComponent::RightY,
    )
}
/// `math.movehl(float4 a, float4 b)` — the high halves in blocks: `a2 a3 b2 b3`.
#[inline]
pub fn movehl(a: float4, b: float4) -> float4 {
    shuffle(
        a,
        b,
        ShuffleComponent::LeftZ,
        ShuffleComponent::LeftW,
        ShuffleComponent::RightZ,
        ShuffleComponent::RightW,
    )
}
/// `math.unpacklo(float4 a, float4 b)` — interleaved low halves: `a0 b0 a1 b1`.
#[inline]
pub fn unpacklo(a: float4, b: float4) -> float4 {
    shuffle(
        a,
        b,
        ShuffleComponent::LeftX,
        ShuffleComponent::RightX,
        ShuffleComponent::LeftY,
        ShuffleComponent::RightY,
    )
}
/// `math.unpackhi(float4 a, float4 b)` — interleaved high halves: `a2 b2 a3 b3`.
#[inline]
pub fn unpackhi(a: float4, b: float4) -> float4 {
    shuffle(
        a,
        b,
        ShuffleComponent::LeftZ,
        ShuffleComponent::RightZ,
        ShuffleComponent::LeftW,
        ShuffleComponent::RightW,
    )
}

// ---------------------------------------------------------------------------
// half conversions — the two directions the `half` type needs
// src: math.f16tof32_1457 @ 0x612f520, math.f32tof16_1461 @ 0x92b9520.
// f16tof32 is transcribed exactly: shift the mantissa into place, add the exponent bias
// (0x38000000, or 0x70000000 when the half exponent is all-ones), and for a zero half exponent run
// the subnormal fix-up `(bits + 0x800000 as float) - 2^-14` (the constant 6.103515625e-05f at
// 0x18cf28614), then OR in the sign.
// f32tof16 is transcribed from the same pass; see the item for the steps.
// ---------------------------------------------------------------------------

/// `math.f16tof32(int x)` — IEEE binary16 to binary32.
#[inline]
pub fn f16tof32(x: i32) -> f32 {
    let x = x as u32;
    let shifted = (x & 0x7fff) << 13;
    let exp_field = shifted & 0x0f80_0000;
    let bias: u32 = if exp_field == 0x0f80_0000 {
        0x7000_0000
    } else {
        0x3800_0000
    };
    let mut mantissa = f32::from_bits(bias.wrapping_add(shifted));
    if exp_field == 0 {
        mantissa = f32::from_bits(mantissa.to_bits().wrapping_add(0x0080_0000)) - 6.103515625e-05;
    }
    f32::from_bits(((x & 0xffff_8000) << 16) | mantissa.to_bits())
}

/// `math.f32tof16(float x)` — IEEE binary32 to binary16. Mask the magnitude to the 20 bits a half
/// can hold (`0x7FFFF000`), rebias by multiplying by `2^-112` (the constant at 0x18cf2858c), clamp
/// with `min(_, 260042752.0f)` (the constant at 0x18cf29590) so an overflow cannot produce a NaN
/// bit pattern, round by adding `0x1000` to the bits and shifting right 13, and take Inf/NaN from
/// the magnitude test the body performs on the masked bits. The result is *not* masked to 16 bits:
/// the original returns 17-bit values for inputs above the half range.
#[inline]
pub fn f32tof16(x: f32) -> u32 {
    const INFINITY_32: i32 = 255 << 23; // 0x7F800000
    const MSK: u32 = 0x7FFF_F000;
    let ux = x.to_bits();
    let uux = ux & MSK;
    let scaled = min(f32::from_bits(uux) * 1.925929944387236e-34, 260042752.0);
    let mut h = scaled.to_bits().wrapping_add(0x1000) >> 13;
    if (uux as i32) >= INFINITY_32 {
        h = if (uux as i32) > INFINITY_32 {
            0x7e00 // NaN -> quiet NaN
        } else {
            0x7c00 // Inf -> Inf
        };
    }
    h | (ux & !MSK) >> 16
}

// ---------------------------------------------------------------------------
// hyperbolics
// src: math.sinh_946 @ 0x92a9..., math.cosh_..., math.tanh_940 @ 0x92a92c0. Each is
// `(float)System.Math.<Name>((float)x)`; the vector overloads (`tanh_941 @ 0x92a9320`,
// `tanh_942 @ 0x92a9360` and friends) call the scalar once per component, which is why the dump
// lists one member name per function. The port keeps the scalar; a vector caller maps it the same
// way.
// ---------------------------------------------------------------------------

/// `math.sinh(float x)` — `(float)System.Math.Sinh((float)x)`.
#[inline]
pub fn sinh(x: f32) -> f32 {
    x.sinh()
}

/// `math.cosh(float x)` — `(float)System.Math.Cosh(x)`.
#[inline]
pub fn cosh(x: f32) -> f32 {
    x.cosh()
}

/// `math.tanh(float x)` — `(float)System.Math.Tanh(x)`.
#[inline]
pub fn tanh(x: f32) -> f32 {
    x.tanh()
}

// ---------------------------------------------------------------------------
// fastinverse
// src: math.fastinverse(float4x4) @ 0x929eac0
// ---------------------------------------------------------------------------

/// `math.fastinverse(float4x4 m)` — the inverse of a matrix assumed to be a rigid transform,
/// transcribed from the body at RVA 0x929eac0.
///
/// The body is an SSE shuffle unroll: 24 `select_shuffle_component` calls pick the twelve rotation
/// elements out of `m.c0`, `m.c1` and `m.c2` (with `0.0` for the fourth element of each result
/// column), and the translation is three dot products against `m.c3.xyz`, each negated by flipping
/// the sign bit. Read through, the shuffles say exactly "transpose the rotation, then negate the
/// matrix applied to the translation column", and the terms of each dot product come out in the
/// order `y, x, z` — that order is kept here because it is what the shipped code computes.
#[inline]
pub fn fastinverse(m: crate::float4x4) -> crate::float4x4 {
    let (c0, c1, c2, c3) = (m.c0, m.c1, m.c2, m.c3);
    let tx = -((c3.y * c0.y + c3.x * c0.x) + c3.z * c0.z);
    let ty = -((c3.y * c1.y + c3.x * c1.x) + c3.z * c1.z);
    let tz = -((c3.y * c2.y + c3.x * c2.x) + c3.z * c2.z);
    crate::float4x4::new(
        float4::new(c0.x, c1.x, c2.x, 0.0),
        float4::new(c0.y, c1.y, c2.y, 0.0),
        float4::new(c0.z, c1.z, c2.z, 0.0),
        float4::new(tx, ty, tz, 1.0),
    )
}
