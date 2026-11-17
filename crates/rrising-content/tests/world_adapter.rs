//! Requires the pinned original installation and RRISING_LAYOUT_REPORT; not a portable CI oracle.
use rrising_content::{
    entity_scenes::SceneFile,
    type_layouts::{TypeLayouts, LOCAL_TO_WORLD_HASH, LOCAL_TRANSFORM_HASH, TRANSLATION_HASH},
    world_adapter::build_scene_world,
};
use rrising_ecs::EntityQuery;
use std::{path::PathBuf, time::Instant};

#[test]
#[ignore = "requires the pinned original installation and RRISING_LAYOUT_REPORT"]
fn all_scenes_build_worlds_with_every_entity() {
    let report = std::env::var_os("RRISING_LAYOUT_REPORT").expect("set RRISING_LAYOUT_REPORT");
    let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("set VRISING_DATA"));
    let layouts = TypeLayouts::from_json(&std::fs::read(report).expect("report")).expect("layouts");
    let mut paths: Vec<_> = std::fs::read_dir(data.join("StreamingAssets/EntityScenes"))
        .expect("scenes")
        .map(|p| p.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "entities"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 1183);
    let (mut entities, mut live, mut chunks_in, mut chunks_built) = (0usize, 0usize, 0, 0);
    let mut counts = [0usize; 3];
    let mut build_time = std::time::Duration::ZERO;
    for path in &paths {
        let scene = SceneFile::open(path).expect("scene");
        let t = Instant::now();
        let sw = build_scene_world(&layouts, &scene)
            .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        build_time += t.elapsed();
        entities += sw.stats.entities;
        chunks_in += sw.stats.chunks;
        assert_eq!(sw.world.entities.len(), sw.stats.entities);
        for e in &sw.world.entities {
            assert!(sw.world.store.exists(*e).expect("exists"), "{e:?}");
        }
        live += sw.world.entities.len();
        chunks_built += sw
            .world
            .store
            .query_chunks(&EntityQuery {
                include_disabled: true,
                include_prefab: true,
                ..Default::default()
            })
            .len();
        for (i, h) in [TRANSLATION_HASH, LOCAL_TRANSFORM_HASH, LOCAL_TO_WORLD_HASH]
            .iter()
            .enumerate()
        {
            if let Some(&t) = sw.type_of_hash.get(h) {
                counts[i] += sw.world.query_count(&EntityQuery {
                    all: vec![t],
                    include_disabled: true,
                    include_prefab: true,
                    ..Default::default()
                });
            }
        }
    }
    println!("world build total {build_time:?}; entities={entities} chunks scene={chunks_in} store={chunks_built} translation/localtransform/localtoworld={counts:?}");
    assert_eq!(entities, 1_385_671);
    assert_eq!(live, 1_385_671);
    assert!(counts.iter().any(|&c| c > 0));
}
