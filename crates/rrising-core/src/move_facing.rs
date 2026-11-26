//! Bounded direction branch from server `CalculateDirectionAndRotationSpeed`.
//! Other facing, rotation speed and command playback are not implemented here.

/// Return the recovered direction when the movement-input branch is eligible.
/// Non-finite inputs and lengths are left to the unimplemented original path.
pub fn movement_facing(input: [f32; 2], rotate_towards_mouse: bool) -> Option<[f32; 3]> {
    let [x, z] = input;
    let predicate = z * z + x * x;
    if rotate_towards_mouse
        || !x.is_finite()
        || !z.is_finite()
        || !predicate.is_finite()
        || predicate <= 0.0
    {
        return None;
    }
    // Preserve the original scalar operation order: x² + 0 then + z².
    let length_sq = (x * x + 0.0) + z * z;
    let inverse = 1.0 / length_sq.sqrt();
    Some([x * inverse, 0.0 * inverse, z * inverse])
}
