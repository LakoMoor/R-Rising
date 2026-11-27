//! TilePosition height decoding, verified against isolated pinned client leaf functions.

pub fn height(compressed: u16) -> f32 {
    f32::from(compressed) * 0.002 - 50.0
}

pub fn height_level(compressed: u16) -> u8 {
    // Preserve the original arithmetic sequence rather than cancelling the offsets.
    (((height(compressed) + 1.0) - -50.0) / 5.0).clamp(0.0, 20.0) as u8
}

/// Serialized logical tile placement; padding is not part of its state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TilePosition {
    pub tile: [i32; 2],
    pub rotation: u8,
    pub compressed_height: u16,
}

impl TilePosition {
    pub fn from_bytes(bytes: &[u8; 12]) -> Self {
        Self {
            tile: [
                i32::from_le_bytes(bytes[..4].try_into().expect("fixed size")),
                i32::from_le_bytes(bytes[4..8].try_into().expect("fixed size")),
            ],
            rotation: bytes[8],
            compressed_height: u16::from_le_bytes([bytes[10], bytes[11]]),
        }
    }

    /// Pinned original baked-to-world operation. Out-of-range chunk rotations
    /// leave the coordinate unrotated but still participate in rotation addition.
    pub fn to_world(self, offset: [i32; 2], chunk_rotation: u8) -> Self {
        let [x, z] = self.tile;
        let tile = match chunk_rotation {
            1 => [z, x.wrapping_neg()],
            2 => [x.wrapping_neg(), z.wrapping_neg()],
            3 => [z.wrapping_neg(), x],
            _ => [x, z],
        };
        Self {
            tile: [
                tile[0].wrapping_add(offset[0]),
                tile[1].wrapping_add(offset[1]),
            ],
            rotation: self.rotation.wrapping_add(chunk_rotation) & 3,
            compressed_height: self.compressed_height,
        }
    }
}
