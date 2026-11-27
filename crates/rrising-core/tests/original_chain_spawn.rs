#![cfg(feature = "content")]
//! An actual chain definition and admitted linked prefab projection, with supplied host placement.
use rrising_content::{
    entity_scenes::SceneFile,
    prefab_catalog::{ScenePrefabCatalog, LINKED_ENTITY_GROUP_HASH, SERVER_PREFAB_SCENES},
    spawn_chain::read_scene_chains,
    type_layouts::TypeLayouts,
};
use rrising_core::{
    chain_attachment::{AttachmentTypes, StaticAttachmentTypes},
    chain_spawn::{
        content::{spawn_linked, PoseTypes, SpawnRequest},
        SpawnPose,
    },
    transition_selection::PrefabCandidate,
};
use rrising_ecs::{Entity, SpecialTypes, TypeIndex, TypeManager, TypeSpec, World, WorldInput};
fn register(types: &mut TypeManager, layouts: &TypeLayouts, name: &str) -> TypeIndex {
    if let Some(ty) = types.type_index_of(name) {
        return ty;
    }
    types
        .register(
            layouts
                .get_by_name(name)
                .unwrap_or_else(|| panic!("layout missing {name}"))
                .spec
                .clone(),
        )
        .unwrap()
}
fn handle(bytes: &[u8]) -> Entity {
    Entity::new(
        i32::from_le_bytes(bytes[..4].try_into().unwrap()),
        i32::from_le_bytes(bytes[4..8].try_into().unwrap()),
    )
}
#[test]
#[ignore = "requires pinned server prefab scenes and RRISING_SERVER_LAYOUT_REPORT"]
fn original_chain_prefab_is_cloned_posed_and_attached() {
    let data = std::path::PathBuf::from(std::env::var_os("VRISING_SERVER_DATA").unwrap());
    let layouts = TypeLayouts::from_server_json(
        &std::fs::read(std::env::var_os("RRISING_SERVER_LAYOUT_REPORT").unwrap()).unwrap(),
    )
    .unwrap();
    let directory = data.join("StreamingAssets/EntityScenes");
    let mut locations = std::collections::BTreeMap::new();
    let mut chains = std::collections::BTreeMap::new();
    for (id, _) in SERVER_PREFAB_SCENES {
        let path = directory.join(format!("{id}.0.entities"));
        let scene = SceneFile::open(&path).unwrap();
        for prefab in ScenePrefabCatalog::read(&scene, &layouts).unwrap().prefabs {
            locations.insert(prefab.prefab_guid, path.clone());
        }
        for (guid, chain) in read_scene_chains(&scene, &layouts).unwrap() {
            chains.insert(guid, chain);
        }
    }
    let pose_names = [
        "Unity.Transforms.Translation",
        "Unity.Transforms.Rotation",
        "Unity.Transforms.LocalTransform",
        "Unity.Transforms.LocalToWorld",
    ];
    let required: Vec<_> = pose_names
        .iter()
        .map(|name| layouts.get_by_name(name).unwrap().stable_hash)
        .collect();
    let mut selected = None;
    let mut missing_pose = 0;
    for (&guid, chain) in &chains {
        let Some(element) = chain.main_element() else {
            continue;
        };
        let Some(path) = locations.get(&element.prefab_guid) else {
            continue;
        };
        let scene = SceneFile::open(path).unwrap();
        let mut catalog = ScenePrefabCatalog::read_linked_templates(
            &scene,
            &layouts,
            &std::collections::BTreeSet::from([element.prefab_guid]),
        )
        .unwrap();
        let Some(template) = catalog.linked_templates.remove(&element.prefab_guid) else {
            continue;
        };
        if required.iter().all(|hash| {
            template.members[0]
                .iter()
                .any(|value| value.stable_hash == *hash)
        }) {
            selected = Some((guid, chain, element, template));
            break;
        }
        missing_pose += 1;
    }
    let (chain_guid, definition, element, template) =
        selected.expect("a real chain target with the original transform slots");
    let mut types = TypeManager::default();
    types.register(TypeSpec::tag("reserved")).unwrap();
    let entity_type = register(&mut types, &layouts, "Unity.Entities.Entity");
    let t = AttachmentTypes {
        child: register(&mut types, &layouts, "ProjectM.SpawnChainChild"),
        disabled: register(&mut types, &layouts, "Unity.Entities.Disabled"),
        active: register(
            &mut types,
            &layouts,
            "ProjectM.SpawnChainData+ActiveChildElement",
        ),
        clock: register(&mut types, &layouts, "ProjectM.AutoChainInstanceData"),
        networked_child: register(&mut types, &layouts, "ProjectM.NetworkedSpawnChainChild"),
        static_compatible: register(&mut types, &layouts, "ProjectM.StaticTransformCompatible"),
        changed_event: register(
            &mut types,
            &layouts,
            "ProjectM.SpawnChainData+ActiveElementsChangedEvent",
        ),
        static_types: Some(StaticAttachmentTypes {
            index: register(&mut types, &layouts, "ProjectM.StaticTransformIndex"),
            mega_compatible: register(&mut types, &layouts, "ProjectM.MegaStaticCompatibleTag"),
            guaranteed_static: register(&mut types, &layouts, "ProjectM.GuaranteedStaticTransform"),
            static_tag: register(&mut types, &layouts, "Unity.Transforms.Static"),
            mega_static: register(&mut types, &layouts, "ProjectM.MegaStaticTag"),
        }),
    };
    let pose_types = PoseTypes {
        translation: register(&mut types, &layouts, pose_names[0]),
        rotation: register(&mut types, &layouts, pose_names[1]),
        local_transform: register(&mut types, &layouts, pose_names[2]),
        local_to_world: register(&mut types, &layouts, pose_names[3]),
    };
    let mut world = World::build(
        types,
        SpecialTypes::new(entity_type),
        32,
        &WorldInput::default(),
    )
    .unwrap();
    let source = template
        .instantiate(&mut world.store, &mut world.types, &layouts)
        .unwrap();
    let arch = world
        .store
        .get_or_create_archetype(
            &world.types,
            &[t.active, t.clock, t.static_types.as_ref().unwrap().index],
        )
        .unwrap();
    let chain = world.store.create_entity(arch).unwrap();
    let candidate = PrefabCandidate {
        element_index: definition.main_index,
        local_to_world: [0.; 16],
        prefab: source[0],
        destroy_chain_on_end: false,
        forwarded_seconds: 1.,
        auto_transition: element
            .use_auto_transition
            .then(|| (&element.auto_transition).into()),
    };
    let pose = SpawnPose::new([1., 2., 3.], [0., 0., 0., 1.]);
    let started = std::time::Instant::now();
    let spawned = spawn_linked(
        &mut world,
        &SpawnRequest {
            chain,
            candidate: &candidate,
            template: &template,
            layouts: &layouts,
            attachment_types: &t,
            pose_types: &pose_types,
            position: [1., 2., 3.],
            rotation: [0., 0., 0., 1.],
            now: 5.,
        },
        |_| None,
    )
    .unwrap();
    let elapsed = started.elapsed();
    assert_eq!(spawned.members.len(), source.len());
    let distinct: std::collections::HashSet<_> = spawned.members.iter().chain(&source).collect();
    assert_eq!(distinct.len(), source.len() * 2);
    let root = spawned.members[0];
    assert_eq!(
        world
            .store
            .component_bytes(root, pose_types.translation)
            .unwrap(),
        pose.translation
    );
    assert_eq!(
        world
            .store
            .component_bytes(root, pose_types.rotation)
            .unwrap(),
        pose.rotation
    );
    assert_eq!(
        world
            .store
            .component_bytes(root, pose_types.local_transform)
            .unwrap(),
        pose.local_transform
    );
    assert_eq!(
        world
            .store
            .component_bytes(root, pose_types.local_to_world)
            .unwrap(),
        pose.local_to_world
    );
    assert!(world
        .store
        .try_component_bytes(root, t.disabled)
        .unwrap()
        .is_some());
    let active = world.store.component_bytes(chain, t.active).unwrap();
    assert_eq!(
        i32::from_le_bytes(active[..4].try_into().unwrap()),
        definition.main_index
    );
    assert_eq!(handle(&active[4..]), root);
    assert_eq!(
        handle(world.store.component_bytes(root, t.child).unwrap()),
        chain
    );
    assert_eq!(
        handle(
            world
                .store
                .component_bytes(spawned.changed_event, t.changed_event)
                .unwrap()
        ),
        chain
    );
    if template.has_group {
        let group = world
            .types
            .infos()
            .iter()
            .find(|info| info.stable_type_hash == LINKED_ENTITY_GROUP_HASH)
            .unwrap()
            .type_index;
        let actual: Vec<_> = world
            .store
            .buffer_payload(&world.types, root, group)
            .unwrap()
            .as_chunks::<8>()
            .0
            .iter()
            .map(|bytes| handle(bytes))
            .collect();
        assert_eq!(actual, spawned.members);
    }
    let before = world
        .store
        .component_bytes(chain, t.active)
        .unwrap()
        .to_vec();
    let bad_template = rrising_content::prefab_catalog::LinkedPrefabTemplate {
        members: vec![Vec::new()],
        has_group: false,
    };
    let rejected = spawn_linked(
        &mut world,
        &SpawnRequest {
            chain,
            candidate: &candidate,
            template: &bad_template,
            layouts: &layouts,
            attachment_types: &t,
            pose_types: &pose_types,
            position: [1., 2., 3.],
            rotation: [0., 0., 0., 1.],
            now: 5.,
        },
        |_| None,
    );
    assert!(rejected
        .err()
        .unwrap()
        .to_string()
        .contains("required transform"));
    assert_eq!(
        world.store.component_bytes(chain, t.active).unwrap(),
        before
    );
    println!("chain_guid={chain_guid} target_guid={} cloned_members={} root_values={} skipped_without_pose={missing_pose} rust_only_spawn_us={}",element.prefab_guid,spawned.members.len(),template.members[0].len(),elapsed.as_micros());
}
