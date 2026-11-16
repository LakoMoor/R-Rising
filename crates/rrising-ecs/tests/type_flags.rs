//! Truth table from shipped getters and BuildComponentType's flag construction.
use rrising_ecs::TypeIndex;

#[test]
fn entity_reference_and_serialization_flags_are_negative_capability_bits() {
    for (raw, refs, serializable) in [
        (1, true, true),
        (1 | 0x20000, false, true),
        (1 | 0x40000, true, false),
        (1 | 0x60000, false, false),
    ] {
        let ty = TypeIndex::new(raw);
        assert_eq!(ty.has_entity_references(), refs, "raw {raw:x}");
        assert_eq!(ty.is_chunk_serializable(), serializable, "raw {raw:x}");
        assert_eq!(ty.index(), 1);
    }
}

#[test]
fn matches_1024_original_native_function_replays() {
    let source = include_str!("../../../tests/golden/type_flags.tsv");
    let mut cases = 0;
    for row in source
        .lines()
        .filter(|row| !row.starts_with('#') && !row.is_empty())
    {
        let parts: Vec<_> = row.split('\t').collect();
        assert_eq!(parts.len(), 3);
        let raw = u32::from_str_radix(parts[0], 16).expect("flags");
        let ty = TypeIndex::new(raw as i32);
        assert_eq!(
            ty.has_entity_references(),
            parts[1] == "1",
            "references flags {raw:08x}"
        );
        assert_eq!(
            ty.is_chunk_serializable(),
            parts[2] == "1",
            "serialization flags {raw:08x}"
        );
        cases += 1;
    }
    assert_eq!(cases, 1024, "missing or incomplete original oracle");
}
