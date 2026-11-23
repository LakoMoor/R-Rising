//! Pinned scene data exercise, not a replay of the original runtime system.
use rrising_content::{
    entity_scenes::SceneFile,
    type_layouts::{TypeLayouts, LOCAL_TRANSFORM_HASH, ROTATION_HASH, TRANSLATION_HASH},
    world_adapter::build_scene_world,
};
use rrising_ecs::{
    transform::{LocalTransformSyncSystem, LocalTransformSyncTypes},
    EntityQuery, Schedule,
};
use std::path::PathBuf;

#[test]
#[ignore = "requires original scenes and RRISING_LAYOUT_REPORT"]
fn syncs_eligible_original_scene_rows() {
    let report = std::env::var_os("RRISING_LAYOUT_REPORT").expect("report");
    let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("data"));
    let layouts = TypeLayouts::from_json(&std::fs::read(report).unwrap()).unwrap();
    let mut rows = 0usize;
    for item in std::fs::read_dir(data.join("StreamingAssets/EntityScenes")).unwrap() {
        let path = item.unwrap().path();
        if !path.extension().is_some_and(|e| e == "entities") {
            continue;
        }
        let scene = SceneFile::open(&path).unwrap();
        let mut sw = build_scene_world(&layouts, &scene).unwrap();
        let ty = |hash| sw.type_of_hash.get(&hash).copied();
        let Some(local) = ty(LOCAL_TRANSFORM_HASH) else {
            continue;
        };
        let types = LocalTransformSyncTypes {
            local,
            translation: ty(TRANSLATION_HASH),
            rotation: ty(ROTATION_HASH),
            scale: ty(0xadc0_56c7_729e_d01e),
            static_tag: ty(0x495a_dd5c_e819_8ca0),
        };
        let query = EntityQuery {
            all: vec![local],
            none: types.static_tag.into_iter().collect(),
            ..Default::default()
        };
        let mut expected = Vec::new();
        for e in sw.world.store.query_entities(&query).unwrap() {
            let mut bytes = sw.world.store.component_bytes(e, local).unwrap().to_vec();
            let pos = types
                .translation
                .and_then(|t| sw.world.store.try_component_bytes(e, t).unwrap());
            let rot = types
                .rotation
                .and_then(|t| sw.world.store.try_component_bytes(e, t).unwrap());
            if pos.is_none() && rot.is_none() {
                continue;
            }
            if let Some(pos) = pos {
                bytes[..12].copy_from_slice(pos);
            }
            if let Some(rot) = rot {
                bytes[16..32].copy_from_slice(rot);
            }
            if let Some(scale) = types
                .scale
                .and_then(|t| sw.world.store.try_component_bytes(e, t).unwrap())
            {
                bytes[12..16].copy_from_slice(scale);
            }
            expected.push((e, bytes));
        }
        let mut schedule = Schedule::new(10).unwrap();
        schedule.add_group("transform").unwrap();
        schedule
            .add_system("transform", Box::new(LocalTransformSyncSystem::new(types)))
            .unwrap();
        schedule.run_frame(&mut sw.world).unwrap();
        for (entity, bytes) in &expected {
            assert_eq!(
                sw.world.store.component_bytes(*entity, local).unwrap(),
                bytes,
                "{} {entity:?}",
                path.display()
            );
        }
        rows += expected.len();
    }
    println!("eligible original scene rows: {rows}");
    assert!(rows > 0);
}
