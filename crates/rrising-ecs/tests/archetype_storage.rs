//! Boundary and ordering cases from CreateArchetype @ RVA 0x8fb0b30.
use rrising_ecs::{
    create_archetype_data, SpecialTypes, TypeCategory, TypeIndex, TypeManager, TypeSpec,
};

#[test]
fn a_chunk_can_exactly_fill_all_32704_payload_bytes() {
    let mut registry = TypeManager::default();
    let mut spec = TypeSpec::component_data("Entity", 8, 8);
    spec.category = TypeCategory::EntityData;
    spec.maximum_chunk_capacity = 4088;
    let entity = registry.register(spec).expect("register");
    let result = create_archetype_data(
        &[entity],
        registry.infos(),
        &SpecialTypes {
            entity_type: entity,
            ..Default::default()
        },
    )
    .expect("layout");
    assert_eq!(result.layout.chunk_capacity, 4088);
    assert_eq!(result.layout.offsets, vec![0]);
}

#[test]
fn memory_order_compares_all_flag_bits_before_the_stable_hash() {
    let mut registry = TypeManager::default();
    let mut a = TypeSpec::component_data("without refs", 4, 4);
    a.flags = TypeIndex::K_HAS_ENTITY_REFERENCES_MASK;
    a.memory_ordering = 1;
    let a = registry.register(a).expect("a");
    let mut b = TypeSpec::component_data("with refs", 4, 4);
    b.memory_ordering = u64::MAX;
    let b = registry.register(b).expect("b");
    let result =
        create_archetype_data(&[a, b], registry.infos(), &SpecialTypes::default()).expect("layout");
    assert_eq!(result.layout.memory_order_to_archetype, vec![1, 0]);
    assert_eq!(result.layout.offsets, vec![16320, 0]);
}
