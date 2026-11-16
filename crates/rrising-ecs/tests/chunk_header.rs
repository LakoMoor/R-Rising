//! Fixed offset evidence, not a writer/reader-only round trip.
//! ArchetypeChunk.get_SequenceNumber @ RVA 0x8ed6720 loads u64 [chunk + 0x28].
use rrising_ecs::{Chunk, Entity, K_HEADER_FIELDS_SIZE, K_SERIALIZED_HEADER_SIZE};

#[test]
fn sequence_number_uses_the_full_u64_at_offset_40() {
    let mut bytes = [0xa5; 64];
    bytes[0..4].copy_from_slice(&7i32.to_le_bytes());
    bytes[8..12].copy_from_slice(&99i32.to_le_bytes());
    bytes[12..16].copy_from_slice(&3i32.to_le_bytes());
    bytes[16..20].copy_from_slice(&128i32.to_le_bytes());
    bytes[28..32].copy_from_slice(&(-1i32).to_le_bytes());
    bytes[32..36].copy_from_slice(&4u32.to_le_bytes());
    bytes[40..48].copy_from_slice(&0x1122334455667788u64.to_le_bytes());
    let header = Chunk::read_serialized_header(&bytes).expect("full chunk header");
    assert_eq!(header.sequence_number, 0x1122334455667788);
    assert_eq!(header.archetype_index, 7);
    assert_eq!(header.meta_chunk_entity, Entity::new(99, 3));
    assert_eq!(header.count, 128);
    assert_eq!(header.list_with_empty_slots_index, -1);
    assert_eq!(header.flags, 4);
}

#[test]
fn a_named_serialized_prefix_is_not_a_full_header_and_short_buffers_are_refused() {
    assert_eq!(K_SERIALIZED_HEADER_SIZE, 40);
    assert_eq!(K_HEADER_FIELDS_SIZE, 48);
    let chunk = Chunk::new(2, 0x1122334455667788);
    for len in 0..48 {
        let mut bytes = vec![0xa5; len];
        assert!(
            Chunk::read_serialized_header(&bytes).is_none(),
            "length {len}"
        );
        assert!(
            chunk.write_serialized_header(&mut bytes).is_none(),
            "length {len}"
        );
        assert!(bytes.iter().all(|b| *b == 0xa5), "short write at {len}");
    }
    let mut bytes = [0xa5; 48];
    chunk
        .write_serialized_header(&mut bytes)
        .expect("48-byte destination");
    assert_eq!(&bytes[36..40], &[0xa5; 4]);
    assert_eq!(&bytes[40..48], &0x1122334455667788u64.to_le_bytes());
}
