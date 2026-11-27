use rrising_core::{
    chain_attachment::plan,
    transition_selection::{AutoConfig, PrefabCandidate},
};
use rrising_ecs::Entity;
use serde_json::json;
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
#[test]
fn original_attachment_command_payloads_match() {
    let data: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evidence/oracles/chain_attachment.json"
    ))
    .unwrap();
    let cases = data["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 432);
    for (row, case) in cases.iter().enumerate() {
        let candidate = PrefabCandidate {
            element_index: case["index"].as_i64().unwrap() as i32,
            local_to_world: [0.; 16],
            prefab: Entity::new(99, 1),
            destroy_chain_on_end: false,
            forwarded_seconds: case["forwarded"].as_f64().unwrap(),
            auto_transition: case["auto"].as_bool().unwrap().then_some(AutoConfig {
                transition_id: [0; 32],
                transition_index: 0,
                attempt_cooldown: 0.,
                duration: case["duration"].as_f64().unwrap() as f32,
            }),
        };
        let result = plan(
            Entity::new(7, 2),
            Entity::new(88, 3),
            &candidate,
            case["networked"].as_bool().unwrap(),
            case["now"].as_f64().unwrap(),
        );
        let mut trace = Vec::new();
        if let Some(clock) = result.clock {
            trace.push(json!({"op":"clock","entity":0x200000007u64,"bytes":hex(&clock)}));
        }
        trace.push(json!({"op":"add_set","entity":0x300000058u64,"types":["disabled","child"]}));
        trace.push(json!({"op":"child","entity":0x300000058u64,"bytes":hex(&result.child)}));
        if let Some(network) = result.networked_child {
            trace.push(json!({"op":"network","entity":0x300000058u64,"bytes":hex(&network)}));
        }
        trace.push(json!({"op":"active","entity":0x200000007u64,"bytes":hex(&result.active)}));
        trace.push(json!({"op":"changed","entity":0x10000007bu64,"bytes":hex(&result.changed)}));
        assert_eq!(json!(trace), case["trace"], "row {row}");
    }
}

#[test]
fn ecs_attachment_updates_links_and_preserves_unwritten_state() {
    use rrising_core::chain_attachment::{apply_existing, AttachmentTypes};
    use rrising_ecs::{SpecialTypes, TypeManager, TypeSpec, World, WorldInput};
    for auto in [false, true] {
        for networked in [false, true] {
            for static_prefab in [false, true] {
                let mut types = TypeManager::default();
                types.register(TypeSpec::tag("reserved")).unwrap();
                let entity_type = types
                    .register(TypeSpec::component_data("Entity", 8, 8))
                    .unwrap();
                let t = AttachmentTypes {
                    disabled: types.register(TypeSpec::tag("Disabled")).unwrap(),
                    child: types
                        .register(TypeSpec::component_data("child", 12, 4))
                        .unwrap(),
                    active: types
                        .register(TypeSpec::component_data("active", 12, 4))
                        .unwrap(),
                    clock: types
                        .register(TypeSpec::component_data("clock", 8, 8))
                        .unwrap(),
                    networked_child: types
                        .register(TypeSpec::component_data("networked", 12, 4))
                        .unwrap(),
                    static_compatible: types
                        .register(TypeSpec::component_data("static", 24, 4))
                        .unwrap(),
                    changed_event: types
                        .register(TypeSpec::component_data("changed", 8, 4))
                        .unwrap(),
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
                    .get_or_create_archetype(&world.types, &[t.active, t.clock])
                    .unwrap();
                let chain = world.store.create_entity(arch).unwrap();
                let mut prefab_types = Vec::new();
                if networked {
                    prefab_types.push(t.networked_child);
                }
                if static_prefab {
                    prefab_types.push(t.static_compatible);
                }
                let arch = world
                    .store
                    .get_or_create_archetype(&world.types, &prefab_types)
                    .unwrap();
                let prefab = world.store.create_entity(arch).unwrap();
                let arch = world
                    .store
                    .get_or_create_archetype(
                        &world.types,
                        &[t.child, t.networked_child, t.disabled],
                    )
                    .unwrap();
                let old_child = world.store.create_entity(arch).unwrap();
                let child = world.store.create_entity(arch).unwrap();
                let mut old_active = [0; 12];
                old_active[4..8].copy_from_slice(&old_child.index.to_le_bytes());
                old_active[8..].copy_from_slice(&old_child.version.to_le_bytes());
                world
                    .store
                    .set_component_bytes(chain, t.active, &old_active)
                    .unwrap();
                world
                    .store
                    .set_component_bytes(chain, t.clock, &42f64.to_le_bytes())
                    .unwrap();
                world
                    .store
                    .set_component_bytes(child, t.networked_child, &[9; 12])
                    .unwrap();
                let candidate = PrefabCandidate {
                    element_index: 1,
                    local_to_world: [0.; 16],
                    prefab,
                    destroy_chain_on_end: false,
                    forwarded_seconds: 1.,
                    auto_transition: auto.then_some(AutoConfig {
                        transition_id: [0; 32],
                        transition_index: 0,
                        attempt_cooldown: 0.,
                        duration: 3.,
                    }),
                };
                let outcome = apply_existing(&mut world, chain, child, &candidate, &t, 10.);
                if static_prefab {
                    assert!(outcome
                        .unwrap_err()
                        .to_string()
                        .contains("static-transform"));
                    assert_eq!(
                        world.store.component_bytes(chain, t.active).unwrap(),
                        old_active
                    );
                    assert_eq!(
                        world.store.component_bytes(chain, t.clock).unwrap(),
                        42f64.to_le_bytes()
                    );
                    assert_eq!(
                        world.store.component_bytes(child, t.child).unwrap(),
                        [0; 12]
                    );
                    continue;
                }
                let event = outcome.unwrap();
                let expected = plan(chain, child, &candidate, networked, 10.);
                assert_eq!(
                    world.store.component_bytes(chain, t.active).unwrap(),
                    expected.active
                );
                assert_eq!(
                    world.store.component_bytes(child, t.child).unwrap(),
                    expected.child
                );
                assert_eq!(
                    world.store.component_bytes(chain, t.clock).unwrap(),
                    if auto { 12f64 } else { 42f64 }.to_le_bytes()
                );
                assert_eq!(
                    world
                        .store
                        .component_bytes(child, t.networked_child)
                        .unwrap(),
                    expected.networked_child.unwrap_or([9; 12])
                );
                assert_eq!(
                    world.store.component_bytes(event, t.changed_event).unwrap(),
                    expected.changed
                );
                assert!(world.store.exists(old_child).unwrap());
                assert!(world.store.exists(prefab).unwrap());
                assert!(world
                    .store
                    .try_component_bytes(child, t.disabled)
                    .unwrap()
                    .is_some());
            }
        }
    }
}
