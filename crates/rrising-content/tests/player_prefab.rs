//! Pinned server data only; this checks the partial prefab projection, not gameplay.
use rrising_content::{
    prefab_catalog::ScenePrefabCatalog,
    type_layouts::{TypeLayouts, ENTITY_HASH},
};
use rrising_ecs::{EntityComponentStore, EntityQuery, SpecialTypes, TypeManager, TypeSpec};
use std::path::PathBuf;

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
    let mut store = EntityComponentStore::new(8, SpecialTypes::new(entity_type)).unwrap();
    let entities = template
        .instantiate(&mut store, &mut types, &layouts)
        .unwrap();
    let mut present = Vec::new();
    for name in [
        "ProjectM.PlayerCharacter",
        "ProjectM.EntityInput",
        "ProjectM.Movement",
        "Unity.Transforms.Translation",
        "Unity.Transforms.LocalToWorld",
    ] {
        let count = types.type_index_of(name).map_or(0, |ty| {
            store
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
    assert!(entities.iter().all(|&e| store.exists(e).unwrap()));
}
