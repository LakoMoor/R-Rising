//! Tests a bounded arithmetic bridge, not a player gameplay simulation.
use rrising_content::{
    entity_scenes::SceneFile, prefab_catalog::ScenePrefabCatalog, type_layouts::TypeLayouts,
    world_adapter::build_scene_world,
};
use rrising_core::ordinary_movement_ecs::{
    step_ordinary_command_bytes, step_ordinary_entity, OrdinaryMovementTypes,
};
use rrising_ecs::EntityQuery;
use std::{collections::BTreeSet, path::PathBuf};

#[test]
#[ignore = "requires pinned client data and RRISING_LAYOUT_REPORT"]
fn bounded_ordinary_step_on_client_player_projection() {
    let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("VRISING_DATA"));
    let layouts = TypeLayouts::from_json(
        &std::fs::read(std::env::var_os("RRISING_LAYOUT_REPORT").expect("RRISING_LAYOUT_REPORT"))
            .unwrap(),
    )
    .unwrap();
    let dir = data.join("StreamingAssets/EntityScenes");
    let prefab_scene =
        SceneFile::open(&dir.join("295c92b7a799a3443b40e9ef192b5727.0.entities")).unwrap();
    let mut catalog = ScenePrefabCatalog::read_linked_templates(
        &prefab_scene,
        &layouts,
        &BTreeSet::from([38526109]),
    )
    .unwrap();
    let template = catalog.linked_templates.remove(&38526109).unwrap();
    let scene = SceneFile::open(&dir.join("c6d34432693f24046ae5916650cc70ef.0.entities")).unwrap();
    let mut world = build_scene_world(&layouts, &scene).unwrap().world;
    let entities = template
        .instantiate(&mut world.store, &mut world.types, &layouts)
        .unwrap();
    let player_type = world
        .types
        .type_index_of("ProjectM.PlayerCharacter")
        .unwrap();
    let player = world
        .store
        .query_entities(&EntityQuery {
            all: vec![player_type],
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .find(|e| entities.contains(e))
        .unwrap();
    let types = OrdinaryMovementTypes::from_world(&world).unwrap();
    let old_move_velocity = world
        .store
        .component_bytes(player, types.move_velocity)
        .unwrap()
        .to_vec();
    let mut movement = world
        .store
        .component_bytes(player, types.movement)
        .unwrap()
        .to_vec();
    assert_eq!(movement[60], 0);
    assert_eq!(movement[100], 0);
    movement[76..80].copy_from_slice(&0.6f32.to_le_bytes());
    movement[80..84].copy_from_slice(&0.8f32.to_le_bytes());
    world
        .store
        .set_component_bytes(player, types.movement, &movement)
        .unwrap();
    let before = world
        .store
        .component_bytes(player, types.translation)
        .unwrap()
        .to_vec();
    step_ordinary_entity(&mut world, player, types, true, 0.25).unwrap();
    let translation = world
        .store
        .component_bytes(player, types.translation)
        .unwrap();
    let read = |b: &[u8]| f32::from_le_bytes(b.try_into().unwrap());
    assert_eq!(
        read(&translation[0..4]),
        read(&before[0..4]) + 0.6f32 * 4.0 * 0.25
    );
    assert_eq!(
        read(&translation[8..12]),
        read(&before[8..12]) + 0.8f32 * 4.0 * 0.25
    );
    assert_eq!(
        &world
            .store
            .component_bytes(player, types.move_velocity)
            .unwrap()[4..],
        &old_move_velocity[4..]
    );
    let first_position = translation.to_vec();
    let mut command = [0u8; rrising_core::command_movement::INPUT_COMMAND_SIZE];
    command[0..4].copy_from_slice(&(-0.5f32).to_le_bytes());
    command[4..8].copy_from_slice(&0.25f32.to_le_bytes());
    command[144..148].copy_from_slice(&0.5f32.to_le_bytes());
    step_ordinary_command_bytes(&mut world, player, types, &command, true).unwrap();
    let position = world
        .store
        .component_bytes(player, types.translation)
        .unwrap();
    assert_eq!(read(&position[0..4]), read(&first_position[0..4]) - 1.0);
    assert_eq!(read(&position[8..12]), read(&first_position[8..12]) + 0.5);
    // Exercise the bounded input -> command -> movement connection on the original
    // prefab. The lookup result is supplied explicitly; no controller or scheduler
    // is simulated and this is not an end-to-end original gameplay comparison.
    let input_type = world.types.type_index_of("ProjectM.EntityInput").unwrap();
    rrising_core::player_input::write_player_movement(
        &mut world,
        player,
        input_type,
        [0.75, -0.25],
    )
    .unwrap();
    let raw = world
        .store
        .component_bytes(player, input_type)
        .unwrap()
        .to_vec();
    let mut command = [0x5au8; rrising_core::command_movement::INPUT_COMMAND_SIZE];
    // With Movement present, original command selection takes its MoveInput,
    // not the distinct raw EntityInput direction.
    rrising_core::command_movement::write_command_input_from_world(
        &world,
        player,
        &mut command,
        input_type,
    )
    .unwrap();
    command[144..148].copy_from_slice(&0.25f32.to_le_bytes());
    let previous = world
        .store
        .component_bytes(player, types.translation)
        .unwrap()
        .to_vec();
    step_ordinary_command_bytes(&mut world, player, types, &command, true).unwrap();
    let after = world
        .store
        .component_bytes(player, types.translation)
        .unwrap();
    assert_eq!(read(&after[0..4]), read(&previous[0..4]) + 0.6 * 4.0 * 0.25);
    assert_eq!(
        read(&after[8..12]),
        read(&previous[8..12]) + 0.8 * 4.0 * 0.25
    );
    assert_eq!(&command[20..116], raw.as_slice());
    // Exercise the recovered ability setup body on the real component. The test
    // supplies eligibility; it does not assert that the original scheduler runs it.
    let mut setup = world
        .store
        .component_bytes(player, types.movement)
        .unwrap()
        .to_vec();
    rrising_core::movement::setup_ability_movement(&mut setup, &raw).unwrap();
    world
        .store
        .set_component_bytes(player, types.movement, &setup)
        .unwrap();
    rrising_core::command_movement::write_command_input_from_world(
        &world,
        player,
        &mut command,
        input_type,
    )
    .unwrap();
    assert_eq!(&command[..8], &raw[..8]);
    let before_setup_step = world
        .store
        .component_bytes(player, types.translation)
        .unwrap()
        .to_vec();
    step_ordinary_command_bytes(&mut world, player, types, &command, true).unwrap();
    let after_setup_step = world
        .store
        .component_bytes(player, types.translation)
        .unwrap();
    assert_eq!(
        read(&after_setup_step[0..4]),
        read(&before_setup_step[0..4]) + 0.75 * 4.0 * 0.25
    );
    assert_eq!(
        read(&after_setup_step[8..12]),
        read(&before_setup_step[8..12]) - 0.25 * 4.0 * 0.25
    );
    movement[60] = 1;
    world
        .store
        .set_component_bytes(player, types.movement, &movement)
        .unwrap();
    assert!(step_ordinary_entity(&mut world, player, types, true, 0.25).is_err());
}
