use rrising_core::player_input::{write_player_command, PlayerCommand};
use rrising_ecs::{
    ArchetypeInput, ChunkInput, Entity, EntityIds, SpecialTypes, TypeCategory, TypeManager,
    TypeSpec, World, WorldInput,
};

#[test]
fn host_command_updates_real_ecs_input_and_preserves_other_fields() {
    let mut types = TypeManager::default();
    types.register(TypeSpec::tag("<reserved null>")).unwrap();
    let mut es = TypeSpec::component_data("Entity", 8, 8);
    es.category = TypeCategory::EntityData;
    let entity_type = types.register(es).unwrap();
    let input_type = types
        .register(TypeSpec::component_data("EntityInput", 96, 4))
        .unwrap();
    let input = WorldInput {
        archetypes: vec![ArchetypeInput {
            types: vec![input_type],
        }],
        chunks: vec![ChunkInput {
            archetype: 0,
            entities: EntityIds::Fresh(1),
            components: vec![(input_type, vec![0x5a; 96])],
            shared_components: vec![],
        }],
    };
    let mut world = World::build(types, SpecialTypes::new(entity_type), 4, &input).unwrap();
    let entity = world.entities[0];
    let command = PlayerCommand {
        movement: [0.25, -0.5],
        aim_position: [1.0, 2.0, 3.0],
        aim_position_plane: [4.0, 5.0, 6.0],
    };
    write_player_command(&mut world, entity, input_type, command).unwrap();
    let bytes = world
        .store
        .component_bytes(entity, input_type)
        .unwrap()
        .to_vec();
    let expected: Vec<_> = command
        .movement
        .into_iter()
        .chain(command.aim_position)
        .chain(command.aim_position_plane)
        .flat_map(f32::to_le_bytes)
        .collect();
    assert_eq!(&bytes[..32], expected);
    assert_eq!(&bytes[32..], &[0x5a; 64]);
    assert!(write_player_command(
        &mut world,
        Entity::new(entity.index, entity.version + 1),
        input_type,
        command
    )
    .is_err());
    assert_eq!(
        world.store.component_bytes(entity, input_type).unwrap(),
        bytes.as_slice()
    );
}
