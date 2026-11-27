#[test]
#[ignore = "requires pinned server scenes and layout report"]
fn original_server_spawns_have_static_placement_keys() {
    use rrising_content::{
        entity_scenes::SceneFile,
        subscene_spawns::{read_spawns, SpawnKind},
        type_layouts::TypeLayouts,
    };
    let data = std::path::PathBuf::from(std::env::var_os("VRISING_SERVER_DATA").unwrap());
    let layouts = TypeLayouts::from_server_json(
        &std::fs::read(std::env::var_os("RRISING_SERVER_LAYOUT_REPORT").unwrap()).unwrap(),
    )
    .unwrap();
    let mut counts = [0; 3];
    for e in std::fs::read_dir(data.join("StreamingAssets/EntityScenes")).unwrap() {
        let path = e.unwrap().path();
        if !path.extension().is_some_and(|x| x == "entities") {
            continue;
        }
        for spawn in read_spawns(&SceneFile::open(&path).unwrap(), &layouts).unwrap() {
            match spawn.kind {
                SpawnKind::Direct => counts[0] += 1,
                SpawnKind::Chain { .. } => counts[1] += 1,
            }
            let (_, index) = spawn.static_index.expect("placement key");
            assert!(index >= 0);
            counts[2] += 1;
        }
    }
    assert_eq!(counts, [1528, 225983, 227511]);
}
