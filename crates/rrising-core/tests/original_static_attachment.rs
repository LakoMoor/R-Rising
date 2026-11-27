#![cfg(feature = "content")]
//! Original prefab bytes + a supplied parent index; not full prefab instantiation or a live world oracle.
use rrising_content::{
    entity_scenes::SceneFile,
    prefab_catalog::{ScenePrefabCatalog, SERVER_PREFAB_SCENES},
    type_layouts::{TypeLayouts, ENTITY_HASH},
};
use rrising_core::{
    chain_attachment::{apply_existing, AttachmentTypes, StaticAttachmentTypes},
    transition_selection::PrefabCandidate,
};
use rrising_ecs::{SpecialTypes, TypeManager, TypeSpec, World, WorldInput};
const COMPATIBLE: u64 = 0x0a5a_2470_72fd_9c4f;
const NETWORK: u64 = 0x7bc1_4c25_58a9_88a6;
const MEGA: u64 = 0x7814_ac88_e25d_caf0;

#[test]
#[ignore = "requires pinned server prefab scenes and RRISING_SERVER_LAYOUT_REPORT"]
fn installed_static_prefab_payloads_attach_to_prepared_children() {
    let data = std::path::PathBuf::from(std::env::var_os("VRISING_SERVER_DATA").unwrap());
    let layouts = TypeLayouts::from_server_json(
        &std::fs::read(std::env::var_os("RRISING_SERVER_LAYOUT_REPORT").unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        layouts.get(COMPATIBLE).unwrap().name,
        "ProjectM.StaticTransformCompatible"
    );
    assert_eq!(
        layouts.get(NETWORK).unwrap().name,
        "ProjectM.NetworkedSpawnChainChild"
    );
    assert_eq!(
        layouts.get(MEGA).unwrap().name,
        "ProjectM.MegaStaticCompatibleTag"
    );
    let mut types = TypeManager::default();
    types.register(TypeSpec::tag("reserved")).unwrap();
    let entity_type = types
        .register(TypeSpec::component_data("Entity", 8, 8))
        .unwrap();
    let bindings = StaticAttachmentTypes {
        index: types
            .register(TypeSpec::component_data("index", 4, 2))
            .unwrap(),
        mega_compatible: types.register(TypeSpec::tag("mega-compatible")).unwrap(),
        guaranteed_static: types.register(TypeSpec::tag("guaranteed")).unwrap(),
        static_tag: types.register(TypeSpec::tag("Static")).unwrap(),
        mega_static: types.register(TypeSpec::tag("mega-static")).unwrap(),
    };
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
        static_types: Some(bindings),
    };
    let bindings = t.static_types.as_ref().unwrap();
    let mut world = World::build(
        types,
        SpecialTypes::new(entity_type),
        32,
        &WorldInput::default(),
    )
    .unwrap();
    let arch = world
        .store
        .get_or_create_archetype(&world.types, &[t.active, t.clock, bindings.index])
        .unwrap();
    let chain = world.store.create_entity(arch).unwrap();
    let index = [0x1f, 0x12, 0x34, 0x56];
    world
        .store
        .set_component_bytes(chain, bindings.index, &index)
        .unwrap();
    let mut rows = 0;
    let mut networked_rows = 0;
    let mut mega_rows = 0;
    let started = std::time::Instant::now();
    let mut application_time = std::time::Duration::ZERO;
    for (guid, _) in SERVER_PREFAB_SCENES {
        let scene =
            SceneFile::open(&data.join(format!("StreamingAssets/EntityScenes/{guid}.0.entities")))
                .unwrap();
        let resolved = layouts.resolve_scene(&scene).unwrap();
        let roots: std::collections::HashSet<_> = ScenePrefabCatalog::read(&scene, &layouts)
            .unwrap()
            .prefabs
            .into_iter()
            .map(|prefab| prefab.entity)
            .collect();
        for chunk in &resolved.chunks {
            for row in 0..chunk.header.count as usize {
                let value = resolved
                    .component_bytes(chunk.index, row, ENTITY_HASH)
                    .unwrap()
                    .unwrap();
                let handle = rrising_ecs::Entity::new(
                    i32::from_le_bytes(value[..4].try_into().unwrap()),
                    i32::from_le_bytes(value[4..].try_into().unwrap()),
                );
                if !roots.contains(&handle) {
                    continue;
                }
                let Some(source) = resolved
                    .component_bytes(chunk.index, row, COMPATIBLE)
                    .unwrap()
                else {
                    continue;
                };
                let networked = resolved
                    .component_bytes(chunk.index, row, NETWORK)
                    .unwrap()
                    .is_some();
                let mega = resolved
                    .component_bytes(chunk.index, row, MEGA)
                    .unwrap()
                    .is_some();
                let application_started = std::time::Instant::now();
                let mut prefab_types = vec![t.static_compatible];
                if networked {
                    prefab_types.push(t.networked_child);
                }
                if mega {
                    prefab_types.push(bindings.mega_compatible);
                }
                let arch = world
                    .store
                    .get_or_create_archetype(&world.types, &prefab_types)
                    .unwrap();
                let prefab = world.store.create_entity(arch).unwrap();
                world
                    .store
                    .set_component_bytes(prefab, t.static_compatible, source)
                    .unwrap();
                let mut child_types = vec![
                    t.static_compatible,
                    t.child,
                    t.disabled,
                    bindings.static_tag,
                    bindings.guaranteed_static,
                ];
                if networked {
                    child_types.push(t.networked_child);
                }
                if mega {
                    child_types.push(bindings.mega_static);
                }
                let arch = world
                    .store
                    .get_or_create_archetype(&world.types, &child_types)
                    .unwrap();
                let child = world.store.create_entity(arch).unwrap();
                let candidate = PrefabCandidate {
                    element_index: 0,
                    local_to_world: [0.; 16],
                    prefab,
                    destroy_chain_on_end: false,
                    forwarded_seconds: 0.,
                    auto_transition: None,
                };
                apply_existing(&mut world, chain, child, &candidate, &t, 0.).unwrap();
                let actual = world
                    .store
                    .component_bytes(child, t.static_compatible)
                    .unwrap();
                assert_eq!(actual[0], 1);
                assert_eq!(&actual[2..6], &index);
                assert_eq!(actual[1], source[1]);
                assert_eq!(&actual[6..], &source[6..]);
                assert_eq!(
                    world
                        .store
                        .component_bytes(prefab, t.static_compatible)
                        .unwrap(),
                    source
                );
                rows += 1;
                networked_rows += usize::from(networked);
                mega_rows += usize::from(mega);
                application_time += application_started.elapsed();
            }
        }
    }
    println!("original_static_prefab_rows={rows} networked={networked_rows} mega_compatible={mega_rows}; supplied parent index, prepared child shapes");
    println!("rust_only_catalog_and_application_ms={} prepared_rows_creation_attachment_assertions_us={}",started.elapsed().as_millis(),application_time.as_micros());
    assert_eq!((rows, networked_rows, mega_rows), (3589, 93, 0));
}
