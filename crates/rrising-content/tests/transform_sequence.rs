//! Compare two Rust simulation paths on pinned scene data; this does not run the original system.
use rrising_content::{
    entity_scenes::SceneFile,
    type_layouts::{
        TypeLayouts, LOCAL_TO_WORLD_HASH, LOCAL_TRANSFORM_HASH, ROTATION_HASH, TRANSLATION_HASH,
    },
    world_adapter::{build_scene_world, PARENT_HASH},
};
use rrising_ecs::{
    transform::{
        LocalTransformSyncSystem, LocalTransformSyncTypes, TransformSystem, TransformTypes,
    },
    EntityQuery, Schedule,
};
use std::path::PathBuf;

#[test]
#[ignore = "requires pinned original scenes and RRISING_LAYOUT_REPORT"]
fn compare_transform_sequences_on_original_scenes() {
    let report = std::env::var_os("RRISING_LAYOUT_REPORT").expect("report");
    let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("data"));
    let layouts = TypeLayouts::from_json(&std::fs::read(report).unwrap()).unwrap();
    let mut counts = [0usize; 5]; // scenes, compared rows, distinct, closer to stored, worse
    for item in std::fs::read_dir(data.join("StreamingAssets/EntityScenes")).unwrap() {
        let path = item.unwrap().path();
        if !path.extension().is_some_and(|e| e == "entities") {
            continue;
        }
        let scene = SceneFile::open(&path).unwrap();
        let mut legacy = build_scene_world(&layouts, &scene).unwrap();
        let Some(&ltw) = legacy.type_of_hash.get(&LOCAL_TO_WORLD_HASH) else {
            continue;
        };
        let mut separated = build_scene_world(&layouts, &scene).unwrap();
        let get = |hash| legacy.type_of_hash.get(&hash).copied();
        let types = TransformTypes {
            local_to_world: ltw,
            local_transform: get(LOCAL_TRANSFORM_HASH),
            translation: get(TRANSLATION_HASH),
            rotation: get(ROTATION_HASH),
            scale: get(0xadc0_56c7_729e_d01e),
            post_transform: get(0x0233_3773_b807_e90d),
            parent: get(PARENT_HASH),
        };
        let query = EntityQuery {
            all: vec![ltw],
            include_disabled: true,
            include_prefab: true,
            ..Default::default()
        };
        let entities = legacy.world.store.query_entities(&query).unwrap();
        let originals: Vec<_> = entities
            .iter()
            .map(|&e| legacy.world.store.component_bytes(e, ltw).unwrap().to_vec())
            .collect();
        let mut old_schedule = Schedule::new(10).unwrap();
        old_schedule.add_group("t").unwrap();
        old_schedule
            .add_system("t", Box::new(TransformSystem::new(types)))
            .unwrap();
        let mut new_schedule = Schedule::new(10).unwrap();
        new_schedule.add_group("t").unwrap();
        if let Some(local) = types.local_transform {
            new_schedule
                .add_system(
                    "t",
                    Box::new(LocalTransformSyncSystem::new(LocalTransformSyncTypes {
                        local,
                        translation: types.translation,
                        rotation: types.rotation,
                        scale: types.scale,
                        static_tag: get(0x495a_dd5c_e819_8ca0),
                    })),
                )
                .unwrap();
        }
        new_schedule
            .add_system("t", Box::new(TransformSystem::local_only(types)))
            .unwrap();
        old_schedule.run_frame(&mut legacy.world).unwrap();
        new_schedule.run_frame(&mut separated.world).unwrap();
        for (e, original) in entities.into_iter().zip(originals) {
            let a = legacy.world.store.component_bytes(e, ltw).unwrap();
            let b = separated.world.store.component_bytes(e, ltw).unwrap();
            counts[1] += 1;
            if a == b {
                continue;
            }
            counts[2] += 1;
            let distance = |bytes: &[u8]| -> f64 {
                bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(original.as_chunks::<4>().0)
                    .map(|(x, y)| {
                        let (x, y) = (f32::from_le_bytes(*x), f32::from_le_bytes(*y));
                        if x.is_finite() && y.is_finite() {
                            f64::from((x - y).abs())
                        } else if x.to_bits() == y.to_bits() {
                            0.0
                        } else {
                            f64::INFINITY
                        }
                    })
                    .sum()
            };
            let (a, b) = (distance(a), distance(b));
            counts[3] += usize::from(b < a);
            counts[4] += usize::from(b > a);
        }
        counts[0] += 1;
    }
    println!("scenes, entities, changed, closer to baked, farther from baked: {counts:?}");
    assert_eq!(counts[0], 372);
    assert!(counts[2] > 0);
}
