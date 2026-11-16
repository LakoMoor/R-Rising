//! Regression scenarios derived from the pinned native bodies, not game-world parity.
use rrising_ecs::{ArchetypeChunkData, ChunkIndex};

#[test]
fn growth_preserves_type_major_versions_and_chunk_major_enable_data() {
    let mut data = ArchetypeChunkData::new(3, 1).expect("metadata");
    data.grow(2).expect("allocate");
    for i in 0..2 {
        data.add(ChunkIndex::new(i + 1), 5 + i, &[100 + i], 7)
            .expect("add");
        data.set_enabled_bits_and_hierarchical_data(
            i as usize,
            &[[1, 2], [3, 4], [5, 6]],
            &[1, 2, 3],
        )
        .expect("masks");
    }
    data.set_order_version(0, 99).expect("order");
    data.set_all_change_version(0, 42).expect("components");
    assert_eq!(data.get_order_version(0).expect("order"), 99);
    assert_eq!(
        data.get_change_version_array_for_type(1).expect("versions"),
        &[42, 7]
    );
    data.grow(5).expect("grow");
    assert_eq!(data.count(), 2);
    assert_eq!(data.capacity(), 5);
    assert_eq!(data.get_shared_component_value(0, 1).expect("shared"), 101);
    assert_eq!(data.get_chunk_entity_count(1).expect("entities"), 6);
    assert_eq!(
        data.get_component_enabled_mask_array_for_type_in_chunk(2, 1)
            .expect("mask"),
        [5, 6]
    );
    assert_eq!(
        data.get_chunk_disabled_count_for_type(1, 1)
            .expect("disabled"),
        2
    );
    assert!(data.grow(5).is_err());
}

#[test]
fn swap_back_moves_all_metadata_and_dispose_retains_type_counts() {
    let mut data = ArchetypeChunkData::new(2, 1).expect("metadata");
    data.grow(3).expect("grow");
    for i in 0..3 {
        let i = i as i32;
        data.add(ChunkIndex::new(i + 1), i + 1, &[100 + i], i as u32)
            .expect("add");
        data.set_enabled_bits_and_hierarchical_data(i as usize, &[[i as u64, 0], [9, 8]], &[i, 4])
            .expect("bits");
    }
    let removal = data.remove_at_swap_back(0).expect("remove");
    assert_eq!(removal.removed, ChunkIndex::new(1));
    assert_eq!(removal.moved, Some(ChunkIndex::new(3)));
    assert_eq!(
        data.chunk_indices(),
        &[ChunkIndex::new(3), ChunkIndex::new(2)]
    );
    assert_eq!(data.get_chunk_entity_count(0).expect("count"), 3);
    assert_eq!(data.get_change_version(1, 0).expect("version"), 2);
    assert_eq!(data.get_shared_component_value(0, 0).expect("shared"), 102);
    assert_eq!(
        data.get_component_enabled_mask_array_for_type_in_chunk(0, 0)
            .expect("mask"),
        [2, 0]
    );
    assert_eq!(
        data.get_chunk_disabled_count_for_type(0, 0)
            .expect("disabled"),
        2
    );
    assert!(data.remove_at_swap_back(2).is_err());
    assert_eq!(data.remove_at_swap_back(1).expect("last").moved, None);
    data.dispose();
    assert_eq!(
        (
            data.count(),
            data.capacity(),
            data.component_count(),
            data.shared_component_count()
        ),
        (0, 0, 2, 1)
    );
    data.grow(1).expect("regrow");
    data.add(ChunkIndex::new(9), 0, &[0], u32::MAX)
        .expect("reuse");
    data.set_chunk_disabled_count_for_type(0, 0, i32::MAX)
        .expect("set");
    data.adjust_chunk_disabled_count_for_type(0, 0, 1)
        .expect("wrap");
    assert_eq!(
        data.get_chunk_disabled_count_for_type(0, 0)
            .expect("wrapped"),
        i32::MIN
    );
}

#[test]
fn aligned_allocation_still_reserves_sixteen_padding_bytes_and_add_requires_capacity() {
    let mut data = ArchetypeChunkData::new(2, 0).expect("metadata");
    assert_eq!(data.buffer_size_for(1).expect("size"), 72); // prefix16 + padding16 + masks/hierarchy40
    assert!(data.add(ChunkIndex::new(1), 0, &[], 0).is_err());
    data.grow(1).expect("grow");
    assert!(data.add(ChunkIndex::NULL, 0, &[], 0).is_err());
    assert!(data.add(ChunkIndex::new(1), 0, &[1], 0).is_err());
    assert_eq!(data.count(), 0);
}
