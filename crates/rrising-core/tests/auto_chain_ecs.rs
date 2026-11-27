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
    let queued_type = types.register(TypeSpec::buffer("queued", 96, 0)).unwrap();
    let local_to_world = types
        .register(TypeSpec::component_data("LocalToWorld", 64, 16))
        .unwrap();
    let child_type = types
        .register(TypeSpec::component_data("SpawnChainChild", 12, 4))
        .unwrap();
    let disabled = types.register(TypeSpec::tag("Disabled")).unwrap();
    let networked_child = types
        .register(TypeSpec::component_data("NetworkedSpawnChainChild", 12, 4))
        .unwrap();
    let static_compatible = types
        .register(TypeSpec::component_data("StaticTransformCompatible", 24, 4))
        .unwrap();
    let changed_event = types
        .register(TypeSpec::component_data("ActiveElementsChangedEvent", 8, 4))
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
        .get_or_create_archetype(
            &world.types,
            &[clock, active, instance, queued_type, local_to_world],
        )
        .unwrap();
    let entity = world.store.create_entity(arch).unwrap();
    let prefab_arch = world
        .store
        .get_or_create_archetype(&world.types, &[])
        .unwrap();
    let prefab = world.store.create_entity(prefab_arch).unwrap();
    let matrix: Vec<u8> = (0..16).flat_map(|i| (i as f32).to_le_bytes()).collect();
    world
        .store
        .set_component_bytes(entity, local_to_world, &matrix)
        .unwrap();
    world
        .store
        .set_component_bytes(entity, clock, &10f64.to_le_bytes())
        .unwrap();
    let mut a = [0; 12];
    a[4..8].copy_from_slice(&99i32.to_le_bytes());
    a[8..].copy_from_slice(&3i32.to_le_bytes());
    world.store.set_component_bytes(entity, active, &a).unwrap();
    let mut id = [0u8; 32];
    id[..2].copy_from_slice(&4u16.to_le_bytes());
    id[2..6].copy_from_slice(b"grow");
    let element = ChainElement {
        index: 0,
        prefab_guid: 1,
        use_auto_transition: true,
        auto_transition: AutoTransitionConfig {
            transition_id: id,
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
    assert_eq!(&bytes[..32], &id);
    assert_eq!(&bytes[32..64], &[0; 32]);
    assert_eq!(&bytes[64..68], &entity.index.to_le_bytes());
    assert_eq!(&bytes[68..72], &entity.version.to_le_bytes());
    assert_eq!(&bytes[72..80], &a[4..12]);
    assert_eq!(&bytes[80..88], &[0; 8]);
    assert_eq!(&bytes[88..], &1f64.to_le_bytes());
    let queue_types = rrising_core::transition_queue::TransitionQueueTypes {
        event,
        active,
        queued: queued_type,
        disabled: None,
    };
    let result = rrising_core::transition_queue::enqueue(&mut world, queued, &queue_types).unwrap();
    assert_eq!(
        result,
        rrising_core::transition_queue::EnqueueResult::Accepted {
            chain: entity,
            replaced: false,
            ready: true
        }
    );
    assert_eq!(
        world
            .store
            .buffer_payload(&world.types, entity, queued_type)
            .unwrap(),
        world.store.component_bytes(queued, event).unwrap()
    );
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
    chain.elements[0].transitions[0].id = id;
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
    rrising_core::transition_queue::enqueue(&mut world, normalized, &queue_types).unwrap();
    let selection = rrising_core::transition_selection::select_queued_primary(
        &world,
        entity,
        0,
        &rrising_core::transition_selection::SelectionTypes {
            queued: queued_type,
            instance,
            local_to_world,
        },
        &chain,
        None,
        |_| Some(prefab),
    )
    .unwrap();
    let candidate = selection.candidate.unwrap();
    assert_eq!(candidate.element_index, 1);
    assert_eq!(candidate.prefab, prefab);
    assert_eq!(candidate.local_to_world, std::array::from_fn(|i| i as f32));
    assert_eq!(candidate.forwarded_seconds.to_bits(), 1f64.to_bits());
    assert_eq!(
        candidate.auto_transition,
        Some((&chain.elements[1].auto_transition).into())
    );
    assert!(!candidate.destroy_chain_on_end);
    // Candidate selection alone must not replace the live child or consume the request.
    assert_eq!(world.store.component_bytes(entity, active).unwrap(), &a);
    assert_eq!(
        world
            .store
            .buffer_payload(&world.types, entity, queued_type)
            .unwrap()
            .len(),
        96
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
    let child_arch = world
        .store
        .get_or_create_archetype(&world.types, &[child_type, disabled])
        .unwrap();
    let new_child = world.store.create_entity(child_arch).unwrap();
    let changed = rrising_core::chain_attachment::apply_existing(
        &mut world,
        entity,
        new_child,
        &candidate,
        &rrising_core::chain_attachment::AttachmentTypes {
            child: child_type,
            disabled,
            active,
            clock,
            networked_child,
            static_compatible,
            changed_event,
        },
        224.,
    )
    .unwrap();
    let attached = rrising_core::chain_attachment::plan(entity, new_child, &candidate, false, 224.);
    assert_eq!(
        world.store.component_bytes(entity, active).unwrap(),
        attached.active
    );
    assert_eq!(
        world.store.component_bytes(new_child, child_type).unwrap(),
        attached.child
    );
    assert_eq!(
        world.store.component_bytes(entity, clock).unwrap(),
        323f64.to_le_bytes()
    );
    assert_eq!(
        world.store.component_bytes(changed, changed_event).unwrap(),
        attached.changed
    );
    assert!(world.store.exists(prefab).unwrap());
}
