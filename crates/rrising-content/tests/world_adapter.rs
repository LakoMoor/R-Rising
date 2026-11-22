//! Requires the pinned original installation and RRISING_LAYOUT_REPORT; not a portable CI oracle.
use rrising_content::{
    entity_scenes::SceneFile,
    type_layouts::{TypeLayouts, LOCAL_TO_WORLD_HASH, LOCAL_TRANSFORM_HASH, TRANSLATION_HASH},
    world_adapter::{build_scene_world, scene_to_world_input, SceneSharedValue},
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
    let mut disabled = 0;
    let mut unmanaged = 0;
    let mut managed = 0;
    let mut build_time = std::time::Duration::ZERO;
    for path in &paths {
        let scene = SceneFile::open(path).expect("scene");
        let t = Instant::now();
        let sw = build_scene_world(&layouts, &scene)
            .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        build_time += t.elapsed();
        let adapted = scene_to_world_input(&layouts, &scene).expect("scene input");
        let shared = scene
            .shared_and_managed_components()
            .expect("shared records");
        unmanaged += shared.unmanaged.len();
        managed += shared.managed_shared_records.len();
        assert_eq!(
            sw.serialized_shared_values.len(),
            shared.serialized_shared_component_count()
        );
        for (stored, source) in sw.serialized_shared_values.iter().zip(&shared.unmanaged) {
            assert_eq!(stored.unmanaged_bytes.as_deref(), Some(source.data));
            assert!(stored.portable);
            assert_eq!(stored.ty, sw.type_of_hash[&source.stable_type_hash]);
        }
        for (stored, source) in sw.serialized_shared_values[shared.unmanaged.len()..]
            .iter()
            .zip(&shared.managed_shared_records)
        {
            assert_eq!(stored.unmanaged_bytes, None);
            assert!(!stored.portable);
            assert_eq!(stored.ty, sw.type_of_hash[&source.stable_type_hash]);
        }
        for source_chunk in &adapted.input.chunks {
            let rrising_ecs::EntityIds::Preserved(ids) = &source_chunk.entities else {
                panic!("scene must preserve ids")
            };
            for &entity in ids {
                let chunk = sw
                    .world
                    .store
                    .entity_in_chunk(entity.index)
                    .expect("entity location")
                    .chunk;
                let stored =
                    &sw.world.store.archetypes()[sw.world.archetype_map[source_chunk.archetype]];
                let list = sw.world.store.chunk(chunk).expect("chunk").list_index as usize;
                let layout = &stored.archetype.data.layout;
                for &(ty, expected) in &source_chunk.shared_components {
                    let shared_types = &stored.archetype.types[layout.first_shared_component
                        as usize
                        ..layout.first_chunk_component as usize];
                    let slot = shared_types
                        .iter()
                        .position(|t| t.type_index == ty)
                        .expect("shared type");
                    let expected_value = if expected == 0 {
                        SceneSharedValue::Default
                    } else if (expected as usize) <= shared.unmanaged.len() {
                        SceneSharedValue::Unmanaged(shared.unmanaged[expected as usize - 1].data)
                    } else {
                        SceneSharedValue::Managed
                    };
                    assert_eq!(
                        sw.serialized_shared_value(entity, ty)
                            .expect("shared lookup"),
                        expected_value
                    );
                    assert_eq!(
                        stored
                            .chunk_data
                            .get_shared_component_value(slot, list)
                            .expect("shared value"),
                        expected
                    );
                }
            }
        }
        entities += sw.stats.entities;
        chunks_in += sw.stats.chunks;
        disabled += sw.stats.disabled_components;
        let mut stored_disabled = 0;
        for archetype in sw.world.store.archetypes() {
            for &slot in &archetype.archetype.enableable_type_index_in_archetype {
                for chunk_index in 0..archetype.chunk_data.count() {
                    stored_disabled += archetype
                        .chunk_data
                        .get_chunk_disabled_count_for_type(slot as usize, chunk_index)
                        .expect("stored disable count")
                        as usize;
                }
            }
        }
        assert_eq!(stored_disabled, sw.stats.disabled_components);
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
    println!("world build total {build_time:?}; entities={entities} chunks scene={chunks_in} store={chunks_built} translation/localtransform/localtoworld={counts:?}; disabled={disabled}");
    assert_eq!(entities, 1_385_671);
    assert_eq!(live, 1_385_671);
    assert_eq!(disabled, 162);
    assert_eq!((unmanaged, managed), (3366, 903));
    assert!(counts.iter().any(|&c| c > 0));
}

#[test]
#[ignore = "requires the pinned original installation and RRISING_LAYOUT_REPORT"]
fn entity_references_resolve_and_parent_chains_are_acyclic() {
    use rrising_content::world_adapter::PARENT_HASH;
    use rrising_ecs::{HierarchyReport, RefCounts};
    use std::collections::BTreeMap;
    let report = std::env::var_os("RRISING_LAYOUT_REPORT").expect("set RRISING_LAYOUT_REPORT");
    let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("set VRISING_DATA"));
    let layouts = TypeLayouts::from_json(&std::fs::read(report).expect("report")).expect("layouts");
    assert_eq!(
        layouts.get(PARENT_HASH).expect("Parent").name,
        "Unity.Transforms.Parent"
    );
    let mut paths: Vec<_> = std::fs::read_dir(data.join("StreamingAssets/EntityScenes"))
        .expect("scenes")
        .map(|p| p.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "entities"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 1183);
    let (mut scalar, mut buffer, mut parent) = (
        RefCounts::default(),
        RefCounts::default(),
        RefCounts::default(),
    );
    let mut h = HierarchyReport::default();
    let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
    let (mut scenes_with_dangling, mut post_patch_live, mut post_patch_nonnull) =
        (0, 0usize, 0usize);
    for path in &paths {
        let scene = SceneFile::open(path).expect("scene");
        let sw = build_scene_world(&layouts, &scene).expect("world");
        let s = &sw.stats;
        for (d, x) in [
            (&mut scalar, s.scalar_refs),
            (&mut buffer, s.buffer_refs),
            (&mut parent, s.parent_refs),
        ] {
            d.null += x.null;
            d.live += x.live;
            d.stale_version += x.stale_version;
            d.dangling += x.dangling;
        }
        if s.unresolved_by_type.values().sum::<usize>() > 0 {
            scenes_with_dangling += 1;
        }
        for (k, v) in &s.unresolved_by_type {
            *by_type.entry(k.clone()).or_default() += v;
        }
        if let Some(&pt) = sw.type_of_hash.get(&PARENT_HASH) {
            let r = sw.world.check_hierarchy(pt).expect("hierarchy");
            h.with_parent += r.with_parent;
            h.null_parent += r.null_parent;
            h.dangling_parent += r.dangling_parent;
            h.cyclic += r.cyclic;
            h.max_depth = h.max_depth.max(r.max_depth);
            // patched bytes must resolve through the live world
            let q = EntityQuery {
                all: vec![pt],
                include_disabled: true,
                include_prefab: true,
                ..Default::default()
            };
            for e in sw.world.store.query_entities(&q).expect("q") {
                let b = sw.world.store.component_bytes(e, pt).expect("bytes");
                let p = rrising_ecs::Entity::new(
                    i32::from_le_bytes(b[0..4].try_into().unwrap()),
                    i32::from_le_bytes(b[4..8].try_into().unwrap()),
                );
                if p != rrising_ecs::Entity::NULL {
                    post_patch_nonnull += 1;
                    if sw.world.resolve(p).is_some() {
                        post_patch_live += 1;
                    }
                }
            }
        }
    }
    println!("scalar={scalar:?} buffer={buffer:?} parent={parent:?} hierarchy={h:?} {by_type:?}");
    let rc = |null, live| RefCounts {
        null,
        live,
        stale_version: 0,
        dangling: 0,
    };
    assert_eq!(scalar, rc(155_086, 141_778));
    assert_eq!(buffer, rc(10_723, 1_491_308));
    assert_eq!(parent, rc(0, 36_200));
    assert!(by_type.is_empty());
    assert_eq!(scenes_with_dangling, 0);
    assert_eq!(
        h,
        HierarchyReport {
            with_parent: 36_200,
            null_parent: 0,
            dangling_parent: 0,
            cyclic: 0,
            max_depth: 18
        }
    );
    assert_eq!((post_patch_live, post_patch_nonnull), (36_200, 36_200));
}
