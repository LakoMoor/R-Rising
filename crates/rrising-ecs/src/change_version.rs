//! `Unity.Entities.ChangeVersionUtility`, verified against the pinned native leaves.
//!
//! Versions form a wrapping u32 counter that skips zero. Ordering is defined by
//! the signed wrapping difference, so callers must observe changes within half
//! the counter range. A last-system version of zero forces the first update.

/// `ChangeVersionUtility.DidChange` (original RVA 0x9072720).
///
/// Zero is the sentinel for a system that has never run: every chunk is changed
/// for that system, including a chunk whose version is also zero.
pub fn did_change(chunk_version: u32, last_system_version: u32) -> bool {
    last_system_version == 0 || (chunk_version.wrapping_sub(last_system_version) as i32) > 0
}

/// `ChangeVersionUtility.IncrementGlobalSystemVersion` (original RVA 0x9072730).
pub fn increment_global_system_version(version: &mut u32) {
    *version = version.wrapping_add(1).max(1);
}
