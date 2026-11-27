//! TilePosition height decoding, verified against isolated pinned client leaf functions.

pub fn height(compressed: u16) -> f32 {
    f32::from(compressed) * 0.002 - 50.0
}

pub fn height_level(compressed: u16) -> u8 {
    // Preserve the original arithmetic sequence rather than cancelling the offsets.
    (((height(compressed) + 1.0) - -50.0) / 5.0).clamp(0.0, 20.0) as u8
}
