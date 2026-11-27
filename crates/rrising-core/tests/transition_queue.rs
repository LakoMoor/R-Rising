use rrising_core::transition_queue::same_key;
#[test]
fn keys_match_original_event_equality() {
    let data: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evidence/oracles/chain_event_key.json"
    ))
    .unwrap();
    let cases = data["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 123);
    let decode = |s: &str| -> [u8; 96] {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    };
    for (index, case) in cases.iter().enumerate() {
        assert_eq!(
            same_key(
                &decode(case["left"].as_str().unwrap()),
                &decode(case["right"].as_str().unwrap())
            )
            .unwrap(),
            case["equal"].as_bool().unwrap(),
            "row {index}"
        );
    }
}
#[test]
fn duplicate_updates_payload_and_stale_child_is_rejected() {
    for disabled_flag in [false, true] {
        use rrising_core::transition_queue::{enqueue, EnqueueResult, TransitionQueueTypes};
        use rrising_ecs::{SpecialTypes, TypeManager, TypeSpec, World, WorldInput};
        let mut types = TypeManager::default();
        types.register(TypeSpec::tag("reserved")).unwrap();
        let entity_type = types
            .register(TypeSpec::component_data("Entity", 8, 8))
            .unwrap();
        let event = types
            .register(TypeSpec::component_data("event", 96, 8))
            .unwrap();
        let active = types
            .register(TypeSpec::component_data("active", 12, 4))
            .unwrap();
        let queued = types.register(TypeSpec::buffer("queued", 96, 0)).unwrap();
        let disabled = types.register(TypeSpec::tag("Disabled")).unwrap();
        let mut world = World::build(
            types,
            SpecialTypes::new(entity_type),
            32,
            &WorldInput::default(),
        )
        .unwrap();
        let mut components = vec![active, queued];
        if disabled_flag {
            components.push(disabled);
        }
        let arch = world
            .store
            .get_or_create_archetype(&world.types, &components)
            .unwrap();
        let chain = world.store.create_entity(arch).unwrap();
        let arch = world
            .store
            .get_or_create_archetype(&world.types, &[event])
            .unwrap();
        let request = world.store.create_entity(arch).unwrap();
        let t = TransitionQueueTypes {
            event,
            active,
            queued,
            disabled: Some(disabled),
        };
        let mut child = [0; 12];
        child[4..8].copy_from_slice(&88i32.to_le_bytes());
        child[8..].copy_from_slice(&3i32.to_le_bytes());
        world
            .store
            .set_component_bytes(chain, active, &child)
            .unwrap();
        let mut bytes = [0u8; 96];
        bytes[..2].copy_from_slice(&4u16.to_le_bytes());
        bytes[2..6].copy_from_slice(b"grow");
        bytes[64..68].copy_from_slice(&chain.index.to_le_bytes());
        bytes[68..72].copy_from_slice(&chain.version.to_le_bytes());
        bytes[72..80].copy_from_slice(&child[4..]);
        world
            .store
            .set_component_bytes(request, event, &bytes)
            .unwrap();
        assert_eq!(
            enqueue(&mut world, request, &t).unwrap(),
            EnqueueResult::Accepted {
                chain,
                replaced: false,
                ready: !disabled_flag
            }
        );
        bytes[88..].copy_from_slice(&2f64.to_le_bytes());
        world
            .store
            .set_component_bytes(request, event, &bytes)
            .unwrap();
        assert_eq!(
            enqueue(&mut world, request, &t).unwrap(),
            EnqueueResult::Accepted {
                chain,
                replaced: true,
                ready: !disabled_flag
            }
        );
        assert_eq!(
            world
                .store
                .buffer_payload(&world.types, chain, queued)
                .unwrap(),
            bytes
        );
        bytes[76..80].copy_from_slice(&2i32.to_le_bytes());
        world
            .store
            .set_component_bytes(request, event, &bytes)
            .unwrap();
        assert_eq!(
            enqueue(&mut world, request, &t).unwrap(),
            EnqueueResult::Rejected
        );
        assert_eq!(
            world
                .store
                .buffer_payload(&world.types, chain, queued)
                .unwrap()
                .len(),
            96
        );
        bytes[68..72].copy_from_slice(&(chain.version + 1).to_le_bytes());
        world
            .store
            .set_component_bytes(request, event, &bytes)
            .unwrap();
        assert_eq!(
            enqueue(&mut world, request, &t).unwrap(),
            EnqueueResult::Rejected
        );
    }
}
