//! Small original helpers verified one by one against replays of the original bytes
//! (`evidence/oracles/small_helpers.tsv`, client RVAs in each doc comment).

/// `ProjectM.SunUtility.IsInSun` (0x7c98bc0): strictly greater than 0.5.
pub fn is_in_sun(sun_intensity: f32) -> bool {
    sun_intensity > 0.5
}

/// `ProjectM.BloodBuffUtility.GetProgress(float, float)` (0x811e700): progress from `threshold` to 100.
pub fn blood_buff_progress(quality: f32, threshold: f32) -> f32 {
    (quality - threshold) / (100.0 - threshold)
}

/// `ProjectM.SoundVolumetric.SoundVolumetricMath.Lerp` (0x7f7cd00): lerp with `t` clamped to
/// [0, 1]. A NaN `t` becomes 1: the original's NaN guard jumps onto the instruction that loads
/// 1.0 (the second clamp then sees a finite value).
pub fn sound_volumetric_lerp(from: [f32; 3], to: [f32; 3], mut t: f32) -> [f32; 3] {
    if t.is_nan() || t > 1.0 {
        t = 1.0;
    }
    if t.is_nan() || t < 0.0 {
        t = 0.0;
    }
    [
        (to[0] - from[0]) * t + from[0],
        (to[1] - from[1]) * t + from[1],
        (to[2] - from[2]) * t + from[2],
    ]
}
