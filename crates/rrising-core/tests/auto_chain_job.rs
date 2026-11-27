#![cfg(feature = "content")]
use rrising_content::spawn_chain::{
    AutoTransitionConfig, ChainDefinition, ChainElement, ChainTransition,
};
use rrising_core::auto_chain_ecs::{apply_entity_with_count, AutoEventOutcome, AutoEventTypes};
use rrising_ecs::{SpecialTypes, TypeManager, TypeSpec, World, WorldInput};
#[test]
fn isolated_job_matches_ecs_clock_counter_and_event() {
    let data: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evidence/oracles/auto_chain_job.json"
    ))
    .unwrap();
    let cases = data["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 800);
    for (row, case) in cases.iter().enumerate() {
        let mut types = TypeManager::default();
        types.register(TypeSpec::tag("reserved")).unwrap();
        let entity_type = types
            .register(TypeSpec::component_data("Entity", 8, 8))
            .unwrap();
        let clock = types
            .register(TypeSpec::component_data("clock", 8, 8))
            .unwrap();
        let active = types
            .register(TypeSpec::component_data("active", 12, 4))
            .unwrap();
        let instance = types
            .register(TypeSpec::component_data("instance", 1, 1))
            .unwrap();
        let event = types
            .register(TypeSpec::component_data("event", 96, 8))
            .unwrap();
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
        let t = AutoEventTypes {
            clock,
            active,
            instance,
            event,
        };
        world
            .store
            .set_component_bytes(
                entity,
                clock,
                &case["deadline"].as_f64().unwrap().to_le_bytes(),
            )
            .unwrap();
        let mut active_bytes = [0u8; 12];
        active_bytes[..4].copy_from_slice(&(case["index"].as_i64().unwrap() as i32).to_le_bytes());
        active_bytes[4..8].copy_from_slice(&88i32.to_le_bytes());
        active_bytes[8..].copy_from_slice(&3i32.to_le_bytes());
        world
            .store
            .set_component_bytes(entity, active, &active_bytes)
            .unwrap();
        let terminal = case["terminal"].as_bool().unwrap();
        let elements: Vec<_> = (0..2)
            .map(|i| ChainElement {
                index: i,
                prefab_guid: 0,
                use_auto_transition: case[if i == 0 { "enabled" } else { "target_auto" }]
                    .as_bool()
                    .unwrap(),
                auto_transition: AutoTransitionConfig {
                    transition_id: [(i + 1) as u8; 32],
                    transition_index: 0,
                    attempt_cooldown: 0.5,
                    duration: if i == 0 { 2. } else { 3. },
                },
                transitions: vec![ChainTransition {
                    id: [0; 32],
                    target: if i == 0 && !terminal { 1 } else { -1 },
                    importance: 0,
                    end_of_chain: if i == 0 { terminal } else { true },
                }],
            })
            .collect();
        let chain = ChainDefinition {
            main_index: 1,
            elements,
        };
        let mut count = case["count"].as_u64().unwrap() as u32;
        let outcome = apply_entity_with_count(
            &mut world,
            entity,
            &chain,
            &t,
            case["now"].as_f64().unwrap(),
            &mut count,
        )
        .unwrap();
        assert_eq!(
            count as u64,
            case["count_after"].as_u64().unwrap(),
            "counter {row}"
        );
        assert_eq!(
            world.store.component_bytes(entity, clock).unwrap(),
            u64::from_str_radix(case["clock_bits"].as_str().unwrap(), 16)
                .unwrap()
                .to_le_bytes(),
            "clock {row}"
        );
        if let Some(hex) = case["event"].as_str() {
            let mut expected: Vec<u8> = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect();
            expected[64..68].copy_from_slice(&entity.index.to_le_bytes());
            expected[68..72].copy_from_slice(&entity.version.to_le_bytes());
            let queued = match outcome {
                AutoEventOutcome::Queued(e) => e,
                other => panic!("missing event {row}: {other:?}"),
            };
            assert_eq!(
                world.store.component_bytes(queued, event).unwrap(),
                expected,
                "payload {row}"
            );
        } else {
            assert!(
                !matches!(outcome, AutoEventOutcome::Queued(_)),
                "unexpected event {row}"
            );
        }
    }
}
