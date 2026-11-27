//! Compare immediate ECS field writes to captured original command payloads.
use rrising_core::{
    chain_attachment::{apply_existing, AttachmentTypes, StaticAttachmentTypes},
    transition_selection::{AutoConfig, PrefabCandidate},
};
use rrising_ecs::{SpecialTypes, TypeManager, TypeSpec, World, WorldInput};
fn decode(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}
#[test]
fn static_world_writes_match_512_original_command_traces() {
    let data: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evidence/oracles/chain_attachment_static.json"
    ))
    .unwrap();
    let cases = data["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 512);
    let mut types = TypeManager::default();
    types.register(TypeSpec::tag("reserved")).unwrap();
    let entity_type = types
        .register(TypeSpec::component_data("Entity", 8, 8))
        .unwrap();
    let t = AttachmentTypes {
        child: types
            .register(TypeSpec::component_data("child", 12, 4))
            .unwrap(),
        disabled: types.register(TypeSpec::tag("Disabled")).unwrap(),
        active: types
            .register(TypeSpec::component_data("active", 12, 4))
            .unwrap(),
        clock: types
            .register(TypeSpec::component_data("clock", 8, 8))
            .unwrap(),
        networked_child: types
            .register(TypeSpec::component_data("network", 12, 4))
            .unwrap(),
        static_compatible: types
            .register(TypeSpec::component_data("compatible", 24, 4))
            .unwrap(),
        changed_event: types
            .register(TypeSpec::component_data("changed", 8, 4))
            .unwrap(),
        static_types: Some(StaticAttachmentTypes {
            index: types
                .register(TypeSpec::component_data("index", 4, 2))
                .unwrap(),
            mega_compatible: types.register(TypeSpec::tag("mega-compatible")).unwrap(),
            guaranteed_static: types.register(TypeSpec::tag("guaranteed")).unwrap(),
            static_tag: types.register(TypeSpec::tag("Static")).unwrap(),
            mega_static: types.register(TypeSpec::tag("mega-static")).unwrap(),
        }),
    };
    let s = t.static_types.as_ref().unwrap();
    let mut world = World::build(
        types,
        SpecialTypes::new(entity_type),
        32,
        &WorldInput::default(),
    )
    .unwrap();
    for (row, case) in cases.iter().enumerate() {
        let trace = case["trace"].as_array().unwrap();
        let requested = trace.iter().find(|op| op["op"] == "add_set").unwrap()["types"]
            .as_array()
            .unwrap();
        let mut parent_types = vec![t.active, t.clock];
        if case["has_index"].as_bool().unwrap() {
            parent_types.push(s.index);
        }
        let arch = world
            .store
            .get_or_create_archetype(&world.types, &parent_types)
            .unwrap();
        let chain = world.store.create_entity(arch).unwrap();
        world
            .store
            .set_component_bytes(chain, t.clock, &42f64.to_le_bytes())
            .unwrap();
        if case["has_index"].as_bool().unwrap() {
            world
                .store
                .set_component_bytes(
                    chain,
                    s.index,
                    &decode(case["static_index"].as_str().unwrap()),
                )
                .unwrap();
        }
        let mut prefab_types = vec![t.static_compatible];
        let networked = case["networked"].as_bool().unwrap();
        if networked {
            prefab_types.push(t.networked_child);
        }
        if case["mega"].as_bool().unwrap() {
            prefab_types.push(s.mega_compatible);
        }
        let arch = world
            .store
            .get_or_create_archetype(&world.types, &prefab_types)
            .unwrap();
        let prefab = world.store.create_entity(arch).unwrap();
        let source = decode(case["source_static"].as_str().unwrap());
        world
            .store
            .set_component_bytes(prefab, t.static_compatible, &source)
            .unwrap();
        let mut child_types = vec![t.child, t.disabled, t.static_compatible];
        if networked {
            child_types.push(t.networked_child);
        }
        for (name, ty) in [
            ("guaranteed_static", s.guaranteed_static),
            ("static", s.static_tag),
            ("mega_static", s.mega_static),
        ] {
            if requested.iter().any(|value| value == name) {
                child_types.push(ty);
            }
        }
        let arch = world
            .store
            .get_or_create_archetype(&world.types, &child_types)
            .unwrap();
        let child = world.store.create_entity(arch).unwrap();
        let candidate = PrefabCandidate {
            element_index: case["index"].as_i64().unwrap() as i32,
            local_to_world: [0.; 16],
            prefab,
            destroy_chain_on_end: false,
            forwarded_seconds: case["forwarded"].as_f64().unwrap(),
            auto_transition: case["auto"].as_bool().unwrap().then_some(AutoConfig {
                transition_id: [0; 32],
                transition_index: 0,
                attempt_cooldown: 0.,
                duration: case["duration"].as_f64().unwrap() as f32,
            }),
        };
        let event = apply_existing(
            &mut world,
            chain,
            child,
            &candidate,
            &t,
            case["now"].as_f64().unwrap(),
        )
        .unwrap();
        for op in trace {
            let (entity, ty) = match op["op"].as_str().unwrap() {
                "clock" => (chain, t.clock),
                "child" => (child, t.child),
                "network" => (child, t.networked_child),
                "static_compatible" => (child, t.static_compatible),
                "active" => (chain, t.active),
                "changed" => (event, t.changed_event),
                "add_set" => continue,
                other => panic!("{other}"),
            };
            let mut expected = decode(op["bytes"].as_str().unwrap());
            // The original sink uses supplied fixed handles. Remap only those handles to this World.
            let replacement = match op["op"].as_str().unwrap() {
                "child" | "network" | "changed" => Some((0, chain)),
                "active" => Some((4, child)),
                _ => None,
            };
            if let Some((offset, handle)) = replacement {
                expected[offset..offset + 4].copy_from_slice(&handle.index.to_le_bytes());
                expected[offset + 4..offset + 8].copy_from_slice(&handle.version.to_le_bytes());
            }
            assert_eq!(
                world.store.component_bytes(entity, ty).unwrap(),
                expected,
                "row {row}, {}",
                op["op"]
            );
        }
        if !case["auto"].as_bool().unwrap() {
            assert_eq!(
                world.store.component_bytes(chain, t.clock).unwrap(),
                42f64.to_le_bytes()
            );
        }
        assert_eq!(
            world
                .store
                .component_bytes(prefab, t.static_compatible)
                .unwrap(),
            source
        );
        assert!(world
            .store
            .try_component_bytes(child, t.disabled)
            .unwrap()
            .is_some());
    }
}
