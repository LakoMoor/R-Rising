//! `Unity.Entities.Hash128` — the 128-bit id every scene stream, blob asset and file carries.
//!
//! Source of truth: `dump.cs` (`Unity.Entities.Hash128`, TypeDefIndex 24087, one `uint4 Value` at
//! 0x0) and the bodies in `../evidence/decompiled/Unity.Entities.dll.c`.
//!
//! The member that matters for the scene format is `StringToHash(char* guidString, int length,
//! bool guidFormatted = true)` @ 0x8fcb550. `SerializeUtility..cctor` @ 0x8fef290 feeds every
//! stream tag through it with `guidFormatted = true`, so the on-disk bytes of a `Hash128` are *not*
//! the plain hex digits of the literal: the compiled parser walks each group of eight characters
//! from the last one to the first, so nibble `7` of the group lands in the top of the `uint` and
//! nibble `0` in the bottom. With the `uint`s then stored little-endian, that reads back as the
//! literal with the two digits of every byte swapped — which is exactly what the shipped
//! `EntityScenes/*.entities` files contain (see `evidence/entity_scenes_fingerprint.txt` and
//! `entity_scenes.rs`).
//!
//! Not ported: `ToString` @ 0x8fcb420 and `Equals(object)` @ 0x8fcb720 (the boxed path), plus the
//! `k_HexToLiteral` / `k_LiteralToHex` tables the compiled code indexes; `parse` here recognises
//! ASCII hex digits directly instead of reproducing the table, which is the same mapping for every
//! input the corpus actually feeds it.

use core::cmp::Ordering;

/// `Hash128.k_GUIDStringLength = 32`.
pub const K_GUID_STRING_LENGTH: usize = 32;

/// `Unity.Entities.Hash128`, one `uint4 Value` at 0x0.
///
/// The four `uint`s are kept in field order, so `value[0]` is `Value.x`. Ordering follows the
/// original, which treats `value[3]` as the most significant word.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct Hash128 {
    /// `Hash128.Value` at 0x0.
    pub value: [u32; 4],
}

impl Hash128 {
    /// `Hash128..ctor(uint x, uint y, uint z, uint w)` @ 0x8fcb2a0.
    pub const fn new(x: u32, y: u32, z: u32, w: u32) -> Self {
        Self {
            value: [x, y, z, w],
        }
    }

    /// The all-zero value `get_IsValid` @ 0x8fcb8d0 compares against (the static `Hash128.Empty`
    /// the body loads from the type's static base).
    pub const NULL: Self = Self {
        value: [0, 0, 0, 0],
    };

    /// `Hash128.get_IsValid` @ 0x8fcb8d0: false only for the all-zero value.
    pub const fn is_valid(&self) -> bool {
        (self.value[0] | self.value[1] | self.value[2] | self.value[3]) != 0
    }

    /// `Hash128.StringToHash` @ 0x8fcb550.
    ///
    /// The compiled body returns the zero hash unless `length == 32`, and treats any character the
    /// hex table rejects as fatal. `guid_formatted` selects the nibble order:
    /// - `true` (what `SerializeUtility` uses): each group of eight characters is read back to
    ///   front, `d[7]<<28 | d[6]<<24 | ... | d[0]`.
    /// - `false`: the pairs are swapped per byte, `d[6]<<28 | d[7]<<24 | d[4]<<20 | d[5]<<16 | ...`,
    ///   which is the mixed-endian layout a `.NET` `Guid` string maps to.
    pub const fn string_to_hash(
        chars: &[u8; K_GUID_STRING_LENGTH],
        guid_formatted: bool,
    ) -> Option<Self> {
        let mut out = [0u32; 4];
        let mut g = 0;
        while g < 4 {
            let mut n = [0u32; 8];
            let mut i = 0;
            while i < 8 {
                match hex_digit(chars[g * 8 + i]) {
                    Some(v) => n[i] = v,
                    None => return None,
                }
                i += 1;
            }
            out[g] = if guid_formatted {
                (n[7] << 28)
                    | (n[6] << 24)
                    | (n[5] << 20)
                    | (n[4] << 16)
                    | (n[3] << 12)
                    | (n[2] << 8)
                    | (n[1] << 4)
                    | n[0]
            } else {
                (n[6] << 28)
                    | (n[7] << 24)
                    | (n[4] << 20)
                    | (n[5] << 16)
                    | (n[2] << 12)
                    | (n[3] << 8)
                    | (n[0] << 4)
                    | n[1]
            };
            g += 1;
        }
        Some(Self { value: out })
    }

    /// `Hash128..ctor(string value)` @ 0x8fcb2d0, which calls `StringToHash` with
    /// `guidFormatted = true`. Returns the zero hash for anything that is not 32 hex characters,
    /// because that is what the compiled body produces.
    pub fn from_guid(s: &str) -> Self {
        let bytes = s.as_bytes();
        if bytes.len() != K_GUID_STRING_LENGTH {
            return Self::NULL;
        }
        let mut buf = [0u8; K_GUID_STRING_LENGTH];
        buf.copy_from_slice(bytes);
        match Self::string_to_hash(&buf, true) {
            Some(h) => h,
            None => Self::NULL,
        }
    }

    /// The 16 bytes as they appear on disk in a `NodeHeader.Id` or a `FileHeader.FileId`: the four
    /// `uint`s written little-endian in field order.
    pub const fn to_bytes(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        let mut g = 0;
        while g < 4 {
            let b = self.value[g].to_le_bytes();
            out[g * 4] = b[0];
            out[g * 4 + 1] = b[1];
            out[g * 4 + 2] = b[2];
            out[g * 4 + 3] = b[3];
            g += 1;
        }
        out
    }

    /// Read the 16 on-disk bytes back into the four `uint`s.
    pub const fn from_bytes(b: &[u8; 16]) -> Self {
        Self {
            value: [
                u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
                u32::from_le_bytes([b[8], b[9], b[10], b[11]]),
                u32::from_le_bytes([b[12], b[13], b[14], b[15]]),
            ],
        }
    }

    /// `Hash128.op_LessThan` @ 0x8fcb7f0: compares `value[3]` first, down to `value[0]`.
    pub const fn less_than(&self, other: &Self) -> bool {
        let mut i = 3;
        loop {
            if self.value[i] != other.value[i] {
                return self.value[i] < other.value[i];
            }
            if i == 0 {
                return false;
            }
            i -= 1;
        }
    }

    /// `Hash128.op_GreaterThan` @ 0x8fcb810: `!(a < b) && !(a == b)` under the same word order.
    pub const fn greater_than(&self, other: &Self) -> bool {
        !self.less_than(other) && !self.eq(other)
    }

    /// `Hash128.GetHashCode` @ 0x8fcb880, kept as the original's wrapping 32-bit arithmetic.
    pub const fn get_hash_code(&self) -> i32 {
        let v = &self.value;
        let a = (v[1] as i32).wrapping_mul(-0x2c8ddf1d);
        let b = (v[3] as i32).wrapping_mul(-0x1e943763);
        let c = (v[2] as i32).wrapping_mul(0x7aa2c2bd);
        let d = (v[0] as i32).wrapping_mul(-0x4b6d40eb);
        a.wrapping_add(b)
            .wrapping_add(c)
            .wrapping_add(d)
            .wrapping_add(0x7aa07cd3)
    }

    /// `Hash128.CompareTo` @ 0x8fcb830.
    pub const fn compare_to(&self, other: &Self) -> Ordering {
        let mut i = 3;
        loop {
            if self.value[i] != other.value[i] {
                return if self.value[i] < other.value[i] {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
            if i == 0 {
                return Ordering::Equal;
            }
            i -= 1;
        }
    }

    const fn eq(&self, other: &Self) -> bool {
        self.value[0] == other.value[0]
            && self.value[1] == other.value[1]
            && self.value[2] == other.value[2]
            && self.value[3] == other.value[3]
    }
}

impl PartialOrd for Hash128 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Hash128 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare_to(other)
    }
}

/// The `k_LiteralToHex` lookup the compiled `StringToHash` indexes; `-1` for anything the table
/// does not map. The table itself is not in the corpus, so this is the ASCII hex mapping it holds
/// for every character the corpus actually feeds it.
const fn hex_digit(c: u8) -> Option<u32> {
    match c {
        b'0'..=b'9' => Some((c - b'0') as u32),
        b'a'..=b'f' => Some((c - b'a') as u32 + 10),
        b'A'..=b'F' => Some((c - b'A') as u32 + 10),
        _ => None,
    }
}
