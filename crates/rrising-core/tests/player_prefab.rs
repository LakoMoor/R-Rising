//! Pinned server data only; this checks the partial prefab projection, not gameplay.
use rrising_content::{
    prefab_catalog::ScenePrefabCatalog,
    type_layouts::{TypeLayouts, ENTITY_HASH},
};
use rrising_core::{
    movement::CharacterMovementFields,
    player_input::{write_player_command, PlayerCommand},
};
use rrising_ecs::{EntityQuery, SpecialTypes, TypeManager, TypeSpec, World, WorldInput};
use std::path::PathBuf;

#[test]
#[ignore = "requires pinned client data and RRISING_LAYOUT_REPORT"]
fn client_player_template_can_be_instantiated_in_scene_world() {
    use rrising_content::{entity_scenes::SceneFile, world_adapter::build_scene_world};
    use std::collections::BTreeSet;

    let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("VRISING_DATA"));
    let layouts = TypeLayouts::from_json(
        &std::fs::read(std::env::var_os("RRISING_LAYOUT_REPORT").expect("RRISING_LAYOUT_REPORT"))
            .unwrap(),
    )
    .unwrap();
    let directory = data.join("StreamingAssets/EntityScenes");
    let prefabs =
        SceneFile::open(&directory.join("295c92b7a799a3443b40e9ef192b5727.0.entities")).unwrap();
    let mut catalog =
        ScenePrefabCatalog::read_linked_templates(&prefabs, &layouts, &BTreeSet::from([38526109]))
            .unwrap();
    let template = catalog
        .linked_templates
        .remove(&38526109)
        .expect("player prefab");
    let scene =
        SceneFile::open(&directory.join("c6d34432693f24046ae5916650cc70ef.0.entities")).unwrap();
    let mut scene_world = build_scene_world(&layouts, &scene).unwrap();
    let world = &mut scene_world.world;
    let entities = template
        .instantiate(&mut world.store, &mut world.types, &layouts)
        .unwrap();
    let player_type = world
        .types
        .type_index_of("ProjectM.PlayerCharacter")
        .unwrap();
    let input_type = world.types.type_index_of("ProjectM.EntityInput").unwrap();
    let players: Vec<_> = world
        .store
        .query_entities(&EntityQuery {
            all: vec![player_type, input_type],
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .filter(|e| entities.contains(e))
        .collect();
    println!(
        "client prefab: {} linked entities, {} live player inputs",
        entities.len(),
        players.len()
    );
    assert_eq!(players.len(), 1);
    assert_eq!(
        world
            .store
            .component_bytes(players[0], input_type)
            .unwrap()
            .len(),
        96
    );
}

#[test]
#[ignore = "requires pinned server data and RRISING_SERVER_LAYOUT_REPORT"]
fn player_template_projection_contains_input_and_movement() {
    let data = PathBuf::from(std::env::var_os("VRISING_SERVER_DATA").expect("VRISING_SERVER_DATA"));
    let layouts = TypeLayouts::from_server_json(
        &std::fs::read(
            std::env::var_os("RRISING_SERVER_LAYOUT_REPORT").expect("RRISING_SERVER_LAYOUT_REPORT"),
        )
        .unwrap(),
    )
    .unwrap();
    let template =
        ScenePrefabCatalog::read_server_linked_prefab(&data, &layouts, 38526109).unwrap();
    let mut types = TypeManager::default();
    types.register(TypeSpec::tag("<reserved>")).unwrap();
    let entity_type = types
        .register(layouts.get(ENTITY_HASH).unwrap().spec.clone())
        .unwrap();
    let mut world = World::build(
        types,
        SpecialTypes::new(entity_type),
        8,
        &WorldInput::default(),
    )
    .unwrap();
    let entities = template
        .instantiate(&mut world.store, &mut world.types, &layouts)
        .unwrap();
    let mut present = Vec::new();
    for name in [
        "ProjectM.PlayerCharacter",
        "ProjectM.EntityInput",
        "ProjectM.Movement",
        "Unity.Transforms.Translation",
        "Unity.Transforms.LocalToWorld",
    ] {
        let count = world.types.type_index_of(name).map_or(0, |ty| {
            world
                .store
                .query_entities(&EntityQuery {
                    all: vec![ty],
                    ..Default::default()
                })
                .unwrap()
                .len()
        });
        present.push((name, count));
    }
    println!(
        "instantiated {} entities, selected components: {present:?}",
        entities.len()
    );
    assert_eq!(entities.len(), 3);
    assert!(present.iter().all(|(_, count)| *count == 1));
    let input = world.types.type_index_of("ProjectM.EntityInput").unwrap();
    let player = world
        .store
        .query_entities(&EntityQuery {
            all: vec![input],
            ..Default::default()
        })
        .unwrap()[0];
    let movement = world.types.type_index_of("ProjectM.Movement").unwrap();
    let fields =
        CharacterMovementFields::read(world.store.component_bytes(player, movement).unwrap())
            .unwrap();
    println!("player prefab movement defaults: {fields:?}");
    assert_eq!(fields.move_input, [0., 0.]);
    assert_eq!(fields.last_nonzero_move_input, [1., 0.]);
    assert_eq!(
        (
            fields.speed,
            fields.ability_cast_speed_multiplier,
            fields.has_move_stop_data
        ),
        (4., 1., false)
    );
    let before = world.store.component_bytes(player, input).unwrap().to_vec();
    let command = PlayerCommand {
        movement: [0.25, -0.5],
        aim_position: [1., 2., 3.],
        aim_position_plane: [4., 5., 6.],
    };
    write_player_command(&mut world, player, input, command).unwrap();
    let after = world.store.component_bytes(player, input).unwrap();
    assert_eq!(&after[..4], &0.25f32.to_le_bytes());
    assert_eq!(&after[4..8], &(-0.5f32).to_le_bytes());
    assert_eq!(&after[32..], &before[32..]);
    assert!(entities.iter().all(|&e| world.store.exists(e).unwrap()));
}
