#[test]
#[ignore = "requires the pinned client and layout report"]
fn installed_tile_definitions_resolve_without_guid_conflicts() {
    use rrising_content::{
        entity_scenes::SceneFile, tile_catalog::TileTemplateCatalog, type_layouts::TypeLayouts,
    };
    let data = std::path::PathBuf::from(std::env::var_os("VRISING_DATA").unwrap());
    let layouts = TypeLayouts::from_json(
        &std::fs::read(std::env::var_os("RRISING_LAYOUT_REPORT").unwrap()).unwrap(),
    )
    .unwrap();
    let mut catalog = TileTemplateCatalog::default();
    for entry in std::fs::read_dir(data.join("StreamingAssets/EntityScenes")).unwrap() {
        let path = entry.unwrap().path();
        if !path.extension().is_some_and(|e| e == "entities") {
            continue;
        }
        let scene = SceneFile::open(&path).unwrap();
        catalog.read_scene(&scene, &layouts).unwrap();
    }
    assert_eq!(catalog.len(), 4553);
    assert!(!catalog.is_empty());
    assert!(catalog.get(i32::MIN).is_none());
}
