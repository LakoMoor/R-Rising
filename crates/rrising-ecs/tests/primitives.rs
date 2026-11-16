//! The ECS primitives, checked against the behaviour the decompiled bodies describe.
//!
//! These are not generic unit tests: each one asserts a rule that is visible in
//! `../evidence/decompiled/Unity.Entities.dll.c`, so a change here that breaks parity with the
//! shipped build has to be a deliberate one.

use std::cmp::Ordering;

use rrising_ecs::{BufferHeader, Entity, K_MINIMUM_CAPACITY, TrashMode};

/// A 4-byte element at 4-byte alignment — the sizes `BufferHeader` is called with for most buffers.
const ELEMENT: i32 = 4;
const ALIGN: i32 = 4;

/// The inline block a header's owner would provide. Sized well past any capacity used here.
fn inline_block() -> [u8; 4096] {
    [0u8; 4096]
}

#[test]
fn null_is_the_zero_handle() {
    // `Entity.ToString` @ 0x8fc5b20 branches on `Index == 0 && Version == 0`; that pair is Null.
    assert_eq!(Entity::NULL.index, 0);
    assert_eq!(Entity::NULL.version, 0);
}

#[test]
fn equality_needs_both_fields() {
    // src: op_Equality @ 0x784e1b0 — compare Index, and only if it matches compare Version.
    let a = Entity::new(7, 1);
    assert_eq!(a, Entity::new(7, 1));
    assert_ne!(a, Entity::new(7, 2), "same slot, different generation");
    assert_ne!(a, Entity::new(8, 1));
    assert_ne!(Entity::NULL, a);
}

#[test]
fn ordering_uses_index_only() {
    // src: CompareTo @ 0x8fc5a80 — `return *param_1 - param_2;`, the indices and nothing else.
    let low = Entity::new(3, 99);
    let high = Entity::new(4, 0);
    assert!(low < high);
    assert_eq!(low.compare_to(high), -1);
    assert_eq!(high.compare_to(low), 1);

    // The original's `CompareTo` ignores Version while `Equals` does not, so same-slot handles
    // order as equal but compare unequal. Tested rather than tidied away.
    let same_slot = Entity::new(3, 1);
    assert_eq!(low.cmp(&same_slot), Ordering::Equal);
    assert_eq!(low.compare_to(same_slot), 0);
    assert_ne!(low, same_slot);
}

#[test]
fn growth_follows_the_bodies_rule() {
    let mut header = BufferHeader::empty();
    let mut inline = inline_block();
    header.initialize(0);
    assert!(header.is_inline(), "Initialize clears Pointer");

    // 0 doubled is 0, so the request wins; then `if (count < 9) count = 8` lifts it to the minimum.
    header
        .ensure_capacity(1, ELEMENT, ALIGN, TrashMode::TrashOldData, false, 0, &mut inline)
        .expect("grow");
    assert_eq!(header.capacity(), K_MINIMUM_CAPACITY);
    assert!(
        !header.is_inline(),
        "EnsureCapacity calls SetCapacity with internalCapacity 0, so growing leaves the inline block"
    );

    // 8 doubled is 16, and a request of 9 fits under that.
    header
        .ensure_capacity(9, ELEMENT, ALIGN, TrashMode::RetainOldData, false, 0, &mut inline)
        .expect("grow");
    assert_eq!(header.capacity(), 16);

    // 16 doubled is 32; a request of 40 is larger, so the request wins.
    header
        .ensure_capacity(40, ELEMENT, ALIGN, TrashMode::RetainOldData, false, 0, &mut inline)
        .expect("grow");
    assert_eq!(header.capacity(), 40);

    // A request already covered changes nothing.
    header
        .ensure_capacity(10, ELEMENT, ALIGN, TrashMode::RetainOldData, false, 0, &mut inline)
        .expect("no-op");
    assert_eq!(header.capacity(), 40);
}

#[test]
fn assign_copies_and_sets_length() {
    // src: Assign @ 0x9120b10 — EnsureCapacity(TrashOldData), MemCpy, then Length = count.
    let mut header = BufferHeader::empty();
    let mut inline = inline_block();
    header.initialize(0);
    let source: Vec<u8> = (0..12u8).collect();
    header
        .assign(&source, 3, ELEMENT, ALIGN, false, 0, &mut inline)
        .expect("assign");
    assert_eq!(header.length(), 3);
    assert_eq!(header.capacity(), K_MINIMUM_CAPACITY);
    assert_eq!(header.data(&inline, ELEMENT), &source[..]);
}

#[test]
fn retain_copies_the_overlap_and_trash_with_a_pattern_memsets() {
    let mut header = BufferHeader::empty();
    let mut inline = inline_block();
    header.initialize(0);
    let source: Vec<u8> = (0..32u8).collect();
    header
        .assign(&source, 8, ELEMENT, ALIGN, false, 0, &mut inline)
        .expect("assign");

    // Grow keeping the old contents: the first eight elements survive the move to a new block.
    header
        .set_capacity(
            16,
            ELEMENT,
            ALIGN,
            TrashMode::RetainOldData,
            false,
            0,
            0,
            &mut inline,
        )
        .expect("grow");
    assert_eq!(header.capacity(), 16);
    assert_eq!(header.data(&inline, ELEMENT), &source[..]);

    // Shrink with an init pattern asked for and TrashOldData: the whole new block is set to the
    // pattern, and Length is left alone — only Assign writes it.
    header
        .set_capacity(4, ELEMENT, ALIGN, TrashMode::TrashOldData, true, 0xAB, 0, &mut inline)
        .expect("shrink");
    assert_eq!(header.capacity(), 4);
    assert_eq!(header.length(), 8, "SetCapacity does not touch Length");
    assert_eq!(header.data(&inline, ELEMENT), &[0xABu8; 16]);
}

#[test]
fn a_header_with_enough_internal_capacity_stays_inline() {
    // src: SetCapacity @ 0x9120950 — `internalCapacity < count` is the test for a heap block. When
    // the inline block is big enough the target and the source are the same block, and the body
    // jumps past the copy and the memset entirely.
    let mut header = BufferHeader::empty();
    let mut inline = [0xEEu8; 4096];
    header.initialize(0);
    header
        .set_capacity(
            8,
            ELEMENT,
            ALIGN,
            TrashMode::TrashOldData,
            true,
            0,
            16,
            &mut inline,
        )
        .expect("set capacity");
    assert!(header.is_inline());
    assert_eq!(header.capacity(), 8);
    assert_eq!(
        &inline[..8 * ELEMENT as usize],
        &[0xEEu8; 32],
        "the same-block path neither copies nor memsets"
    );
}

#[test]
fn destroy_clears_the_header() {
    // src: Destroy @ 0x9120bd0 — free the block, clear Pointer, and zero Length and Capacity with
    // one 8-byte store.
    let mut header = BufferHeader::empty();
    let mut inline = inline_block();
    header.initialize(0);
    header
        .assign(&[1u8; 16], 4, ELEMENT, ALIGN, false, 0, &mut inline)
        .expect("assign");
    assert!(!header.is_inline());
    header.destroy();
    assert!(header.is_inline());
    assert_eq!(header.length(), 0);
    assert_eq!(header.capacity(), 0);
}

#[test]
fn assigning_too_few_source_bytes_is_an_error_not_a_overrun() {
    let mut header = BufferHeader::empty();
    let mut inline = inline_block();
    header.initialize(0);
    let err = header
        .assign(&[0u8; 4], 4, ELEMENT, ALIGN, false, 0, &mut inline)
        .expect_err("4 elements of 4 bytes need 16 source bytes");
    assert!(
        err.to_string().contains("from 4 source bytes"),
        "the message should name what was short: {err}"
    );
}
