#![cfg(feature = "content")]
//! Real baked data acceptance, not a live-steering or gameplay oracle.
use rrising_content::{
    entity_scenes::SceneFile, type_layouts::TypeLayouts, world_adapter::build_scene_world,
};
use rrising_core::{
    movement::{SharedSteeringSystem, SharedSteeringTypes},
    pathfinding::PathWaypoints,
};
use rrising_ecs::{FrameContext, System};

#[test]
#[ignore = "requires the pinned installation and RRISING_LAYOUT_REPORT; baked prefabs are not live steering"]
fn original_movement_templates_remain_excluded_from_the_live_query() {
    let data =
        std::path::PathBuf::from(std::env::var_os("VRISING_DATA").expect("set VRISING_DATA"));
    let report = std::env::var_os("RRISING_LAYOUT_REPORT").expect("set RRISING_LAYOUT_REPORT");
    let layouts = TypeLayouts::from_json(&std::fs::read(report).unwrap()).unwrap();
    let mut paths: Vec<_> = std::fs::read_dir(data.join("StreamingAssets/EntityScenes"))
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "entities"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 1183);
    let (mut templates, mut live, mut scenes) = (0, 0, 0);
    for path in paths {
        let scene = SceneFile::open(&path).unwrap();
        let mut sw = build_scene_world(&layouts, &scene).unwrap();
        let Some(types) = SharedSteeringTypes::from_hashes(&sw.type_of_hash) else {
            continue;
        };
        let query = types.query();
        let candidates = sw.world.store.query_entities(&query).unwrap();
        live += candidates.len();
        let mut prefab_query = query.clone();
        prefab_query.include_prefab = true;
        prefab_query.include_disabled = true;
        let source = sw.world.store.query_entities(&prefab_query).unwrap();
        if source.is_empty() {
            continue;
        }
        scenes += 1;
        let mut just_prefabs = query.clone();
        just_prefabs.include_prefab = true;
        assert_eq!(
            source,
            sw.world.store.query_entities(&just_prefabs).unwrap()
        );
        assert!(
            candidates.is_empty(),
            "{} contains live candidates",
            path.display()
        );
        templates += source.len();
        for e in source {
            let b = sw
                .world
                .store
                .component_bytes(e, types.move_towards_position_buff)
                .unwrap();
            let points = PathWaypoints::from_bytes(&b[16..36]).unwrap();
            assert!(!points.has_waypoint && !points.has_next_waypoint);
            assert_eq!(&b[8..16], &[0; 8]);
            assert_eq!(
                &sw.world.store.component_bytes(e, types.buff).unwrap()[8..16],
                &[0; 8]
            );
        }
        let before = sw.world.state_hash();
        let ctx = FrameContext {
            frame: 0,
            delta_ns: 16_666_667,
            elapsed_ns: 0,
            system_version: 1,
            last_system_version: 0,
        };
        let mut system = SharedSteeringSystem::new(types);
        system.update(&mut sw.world, &ctx).unwrap();
        assert_eq!(system.entities_updated, 0);
        assert_eq!(sw.world.state_hash(), before);
    }
    println!("{scenes} scenes with {templates} baked templates, {live} live steering candidates; no gameplay execution claimed");
    assert_eq!((scenes, templates, live), (2, 32, 0));
}
