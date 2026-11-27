use rrising_content::physics_collider::ColliderHeader;

#[test]
fn common_header_checks_bounds_and_decodes_filter() {
    assert!(ColliderHeader::read(&[0; 19]).is_err());
    let data = [
        5, 0, 2, 0xff, 1, 0, 0, 0, 2, 0, 0, 0, 4, 0, 0, 0, 255, 255, 255, 255,
    ];
    let header = ColliderHeader::read(&data).unwrap();
    assert_eq!(
        (
            header.shape,
            header.collision_type,
            header.version,
            header.magic
        ),
        (5, 0, 2, 255)
    );
    assert_eq!(
        (
            header.unique_id,
            header.belongs_to,
            header.collides_with,
            header.group_index
        ),
        (1, 2, 4, -1)
    );
}

#[test]
#[ignore = "requires the pinned client installation and layout report"]
fn original_diagnostic_scene_collider_references_resolve() {
    use rrising_content::{
        entity_scenes::SceneFile, physics_collider::read_scene_colliders, type_layouts::TypeLayouts,
    };
    let data = std::path::PathBuf::from(std::env::var_os("VRISING_DATA").unwrap());
    let layout = TypeLayouts::from_json(
        &std::fs::read(std::env::var_os("RRISING_LAYOUT_REPORT").unwrap()).unwrap(),
    )
    .unwrap();
    let scene = SceneFile::open(
        &data.join("StreamingAssets/EntityScenes/c6d34432693f24046ae5916650cc70ef.0.entities"),
    )
    .unwrap();
    let colliders = read_scene_colliders(&scene, &layout).unwrap();
    assert_eq!(colliders.len(), 147);
    assert!(colliders
        .iter()
        .all(|v| v.header.is_some_and(|h| h.shape == 8)));
}
