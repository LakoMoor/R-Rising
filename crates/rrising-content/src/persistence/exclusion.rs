//! Saved-component exclusion predicate of the original loader.
//! src: server `PersistenceV2Utility.ShouldUseSavedComponentData` (VA 0x1879922f0), see
//! evidence/save_bindings.md. Observed behavior: bit `type_index & 0x3fff` must be clear in both the
//! saved-excluded and the current-excluded set for the saved data to be used.

/// `ExcludePersistenceOptions` values from the server metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExcludePersistenceOption {
    CopyDataFromPrefab = 0,
    RemoveComponentFromArchetype = 1,
    LogErrorIfComponentSaved = 2,
    SetDataToDefault = 3,
}

/// Mask applied to a TypeIndex before it addresses the exclusion bit arrays.
pub const EXCLUSION_INDEX_MASK: u32 = 0x3fff;

/// Little-endian 64-bit word bit array, as `NativeBitArray` stores it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExclusionSet {
    words: Vec<u64>,
}

impl ExclusionSet {
    pub fn with_capacity_bits(bits: usize) -> Self {
        Self {
            words: vec![0; bits.div_ceil(64)],
        }
    }

    pub fn insert(&mut self, type_index: i32) -> bool {
        let bit = (type_index as u32 & EXCLUSION_INDEX_MASK) as usize;
        match self.words.get_mut(bit / 64) {
            Some(word) => {
                *word |= 1 << (bit % 64);
                true
            }
            None => false,
        }
    }

    /// Out-of-range indices are an error in the original (`CheckArgs`); `None` models that.
    pub fn contains(&self, type_index: i32) -> Option<bool> {
        let bit = (type_index as u32 & EXCLUSION_INDEX_MASK) as usize;
        self.words.get(bit / 64).map(|w| w >> (bit % 64) & 1 == 1)
    }
}

/// Saved component data is applied only when neither set excludes the type.
pub fn should_use_saved_component_data(
    saved_excluded: &ExclusionSet,
    current_excluded: &ExclusionSet,
    type_index: i32,
) -> Option<bool> {
    Some(!saved_excluded.contains(type_index)? && !current_excluded.contains(type_index)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excluded_in_either_set_rejects_saved_data() {
        let mut saved = ExclusionSet::with_capacity_bits(0x4000);
        let mut current = ExclusionSet::with_capacity_bits(0x4000);
        assert_eq!(
            should_use_saved_component_data(&saved, &current, 70),
            Some(true)
        );
        saved.insert(70);
        assert_eq!(
            should_use_saved_component_data(&saved, &current, 70),
            Some(false)
        );
        current.insert(71);
        assert_eq!(
            should_use_saved_component_data(&saved, &current, 71),
            Some(false)
        );
        assert_eq!(
            should_use_saved_component_data(&saved, &current, 72),
            Some(true)
        );
    }

    #[test]
    fn flag_bits_above_the_index_mask_are_ignored() {
        let mut saved = ExclusionSet::with_capacity_bits(0x4000);
        let current = ExclusionSet::with_capacity_bits(0x4000);
        saved.insert(5);
        assert_eq!(
            should_use_saved_component_data(&saved, &current, 5 | 0x4000_0000),
            Some(false)
        );
    }

    #[test]
    fn out_of_range_index_is_reported() {
        let small = ExclusionSet::with_capacity_bits(64);
        assert_eq!(should_use_saved_component_data(&small, &small, 100), None);
    }
}
