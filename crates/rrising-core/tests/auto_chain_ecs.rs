#![cfg(feature = "content")]
use rrising_content::spawn_chain::{
    AutoTransitionConfig, ChainDefinition, ChainElement, ChainTransition,
};
use rrising_core::auto_chain_ecs::{apply_entity, AutoEventOutcome, AutoEventTypes};
use rrising_ecs::{SpecialTypes, TypeManager, TypeSpec, World, WorldInput};
#[test]
fn terminal_event_is_queued_with_original_field_layout() {
    let mut types = TypeManager::default();
    types.register(TypeSpec::tag("reserved")).unwrap();
    let entity_type = types
        .register(TypeSpec::component_data("Entity", 8, 8))
        .unwrap();
    let clock = types
        .register(TypeSpec::component_data("AutoChainInstanceData", 8, 8))
        .unwrap();
    let active = types
        .register(TypeSpec::component_data("ActiveChildElement", 12, 4))
        .unwrap();
    let instance = types
        .register(TypeSpec::component_data("SpawnChainInstance", 1, 1))
        .unwrap();
    let event = types
        .register(TypeSpec::component_data("SpawnChainTransitionEvent", 96, 8))
        .unwrap();
    let t = AutoEventTypes {
        clock,
        active,
        instance,
        event,
    };
    let mut world = World::build(
        types,
        SpecialTypes::new(entity_type),
        32,
        &WorldInput::default(),
    )
    .unwrap();
    let arch = world
        .store
        .get_or_create_archetype(&world.types, &[clock, active, instance])
        .unwrap();
    let entity = world.store.create_entity(arch).unwrap();
    world
        .store
        .set_component_bytes(entity, clock, &10f64.to_le_bytes())
        .unwrap();
    let mut a = [0; 12];
    a[4..8].copy_from_slice(&99i32.to_le_bytes());
    a[8..].copy_from_slice(&3i32.to_le_bytes());
    world.store.set_component_bytes(entity, active, &a).unwrap();
    let element = ChainElement {
        index: 0,
        prefab_guid: 1,
        use_auto_transition: true,
        auto_transition: AutoTransitionConfig {
            transition_id: [7; 32],
            transition_index: 0,
            attempt_cooldown: 2.,
            duration: 100.,
        },
        transitions: vec![ChainTransition {
            id: [7; 32],
            target: -1,
            importance: 0,
            end_of_chain: true,
        }],
    };
    let mut chain = ChainDefinition {
        main_index: 0,
        elements: vec![element],
    };
    let queued = match apply_entity(&mut world, entity, &chain, &t, 11.).unwrap() {
        AutoEventOutcome::Queued(e) => e,
        other => panic!("{other:?}"),
    };
    let bytes = world.store.component_bytes(queued, event).unwrap();
    assert_eq!(&bytes[..32], &[7; 32]);
    assert_eq!(&bytes[32..64], &[0; 32]);
    assert_eq!(&bytes[64..68], &entity.index.to_le_bytes());
    assert_eq!(&bytes[68..72], &entity.version.to_le_bytes());
    assert_eq!(&bytes[72..80], &a[4..12]);
    assert_eq!(&bytes[80..88], &[0; 8]);
    assert_eq!(&bytes[88..], &1f64.to_le_bytes());
    assert_eq!(
        world.store.component_bytes(entity, clock).unwrap(),
        13f64.to_le_bytes()
    );
    assert_eq!(
        apply_entity(&mut world, entity, &chain, &t, 12.).unwrap(),
        AutoEventOutcome::Skipped
    );
    chain.elements.push(chain.elements[0].clone());
    chain.elements[0].transitions[0].target = 1;
    chain.elements[0].transitions[0].end_of_chain = false;
    chain.main_index = 1;
    let normalized = match apply_entity(&mut world, entity, &chain, &t, 14.).unwrap() {
        AutoEventOutcome::Queued(e) => e,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        &world.store.component_bytes(normalized, event).unwrap()[88..],
        &1f64.to_le_bytes()
    );
    assert_eq!(
        world.store.component_bytes(entity, clock).unwrap(),
        16f64.to_le_bytes()
    );
    chain.main_index = 0;
    chain.elements[0].transitions[0].target = 0;
    let wrapped = match apply_entity(&mut world, entity, &chain, &t, 221.).unwrap() {
        AutoEventOutcome::Queued(e) => e,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        &world.store.component_bytes(wrapped, event).unwrap()[88..],
        &5f64.to_le_bytes()
    );
    chain.elements[0].auto_transition.duration = 0.;
    assert_eq!(
        apply_entity(&mut world, entity, &chain, &t, 224.).unwrap(),
        AutoEventOutcome::UnsupportedNormalization
    );
    assert_eq!(
        world.store.component_bytes(entity, clock).unwrap(),
        223f64.to_le_bytes()
    );
}
