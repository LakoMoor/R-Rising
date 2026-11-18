use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{ensure, Context, Result};
use rrising_content::{
    entity_scenes::SceneFile,
    persistence::{MegaStaticRestorePlan, PersistentSave, SavedSnapshot},
    prefab_catalog::{ScenePrefabCatalog, SERVER_PREFAB_SCENES},
    restore_world::RestoredWorldProjection,
    type_layouts::TypeLayouts,
};
use rrising_ecs::Entity;

fn local_path(variable: &str) -> Result<PathBuf> {
    std::env::var_os(variable)
        .map(PathBuf::from)
        .with_context(|| format!("set {variable}"))
}

#[test]
#[ignore = "requires the pinned server data, its layout report and the two original saves"]
fn registrations_recover_every_saved_prefab_from_runtime_selected_scenes() -> Result<()> {
    let data = local_path("VRISING_SERVER_DATA")?.join("StreamingAssets/EntityScenes");
    let layouts = TypeLayouts::from_server_json(&std::fs::read(local_path(
        "RRISING_SERVER_LAYOUT_REPORT",
    )?)?)?;
    let saves = local_path("RRISING_SAVE_DIR")?;
    let mut catalog = BTreeMap::new();
    let mut scene_catalogs = Vec::new();
    for (scene_id, expected_count) in SERVER_PREFAB_SCENES {
        let scene = SceneFile::open(&data.join(format!("{scene_id}.0.entities")))?;
        let roots = ScenePrefabCatalog::read(&scene, &layouts)
            .with_context(|| format!("building catalog for {scene_id}"))?;
        assert_eq!(
            (roots.prefabs.len(), roots.unresolved_rows),
            (expected_count, 0),
            "{scene_id}"
        );
        for root in &roots.prefabs {
            ensure!(
                catalog
                    .insert(root.prefab_guid, root.linked_entities.len())
                    .is_none(),
                "repeated registered GUID {}",
                root.prefab_guid
            );
        }
        scene_catalogs.push(roots);
    }
    assert_eq!(catalog.len(), 23583);
    for number in [4, 5] {
        let save = PersistentSave::open(&saves.join(format!("AutoSave_{number}.save.gz")))?;
        let snapshot = SavedSnapshot::read(&save)?;
        let expected: BTreeMap<_, _> = snapshot
            .header()
            .prefabs
            .iter()
            .map(|prefab| (prefab.prefab_guid, prefab.linked_entities.len()))
            .collect();
        assert_eq!(catalog.len(), expected.len());
        for (guid, linked_count) in expected {
            assert_eq!(
                catalog.get(&guid),
                Some(&linked_count),
                "save {number}, GUID {guid}"
            );
        }
        let restored = RestoredWorldProjection::build(&snapshot, &layouts, &scene_catalogs)?;
        assert_eq!(restored.prefabs.len(), 23583);
        assert_eq!(
            restored.bindings.len(),
            if number == 4 { 315444 } else { 315442 }
        );
        let extended = layouts
            .get(rrising_content::type_layouts::ENTITY_HASH)
            .unwrap()
            .portable_value;
        if extended {
            assert_eq!(restored.admitted_saved_types, 263);
            assert_eq!(
                restored.skipped_saved_types,
                if number == 4 { 20 } else { 18 }
            );
            assert_eq!(
                restored.copied_scalar_values,
                if number == 4 { 1826360 } else { 1826352 }
            );
            assert_eq!(restored.copied_buffer_rows, 276642);
            assert_eq!(
                restored.copied_buffer_elements,
                if number == 4 { 794036 } else { 793963 }
            );
            assert_eq!(restored.cleared_references, 471190);
            let missing = &restored.missing_references;
            assert_eq!(missing.nonpersistent, 471178);
            assert_eq!(missing.stale_version, 12);
            assert_eq!(missing.distinct_handles, 235595);
            assert_eq!((missing.unlisted, missing.outside_world_table), (0, 0));
            assert_eq!(
                missing.by_component["ProjectM.MegaStaticInstanceBuffer"],
                235589
            );
            assert_eq!(
                missing.by_component["ProjectM.SpawnChainData+ActiveChildElement"],
                235595
            );
            assert_eq!(
                missing.by_component["ProjectM.SpawnChainData+QueuedTransitionRequests"],
                6
            );
            let plan = MegaStaticRestorePlan::read(&snapshot)?;
            assert_eq!(
                (
                    plan.manager_rows,
                    plan.entries.len(),
                    plan.vacant_slots,
                    plan.distinct_prefab_guids
                ),
                (3101, 235589, 4216, 1182)
            );
            let mut instances = rustc_hash::FxHashSet::default();
            let mut placement_keys = rustc_hash::FxHashSet::default();
            for entry in &plan.entries {
                assert!(instances.insert(entry.saved_instance));
                assert!(placement_keys.insert((entry.terrain_chunk, entry.transform_index)));
                assert!(restored.bindings.target(entry.saved_instance).is_none());
                for source in [entry.saved_manager, entry.saved_prefab_root] {
                    assert!(restored
                        .store
                        .exists(restored.bindings.target(source).unwrap())?);
                }
            }
            let active_child = snapshot
                .header()
                .persistent_component_named("ProjectM.SpawnChainData+ActiveChildElement")
                .unwrap()
                .saved_type_index;
            for (_, job) in snapshot.jobs() {
                for chunk in job
                    .archetypes
                    .iter()
                    .flat_map(|archetype| &archetype.chunks)
                {
                    let Some(column) = chunk
                        .columns
                        .iter()
                        .find(|column| column.saved_type_index == active_child)
                    else {
                        continue;
                    };
                    for row in 0..chunk.entities().len() {
                        let bytes = column.scalar(row).unwrap();
                        let instance = read_handle(bytes[4..12].try_into().unwrap());
                        instances.remove(&instance);
                    }
                }
            }
            assert!(
                instances.is_empty(),
                "each planned instance must appear as an active spawn-chain child"
            );
            for (name, owner) in [
                ("Unity.Transforms.Translation", Entity::new(40534, 2)),
                ("ProjectM.Velocity", Entity::new(2167, 1)),
            ] {
                let component = snapshot.header().persistent_component_named(name).unwrap();
                let original = snapshot
                    .entity(owner)
                    .unwrap()
                    .scalar(component.saved_type_index)
                    .unwrap();
                let current = restored.bindings.target(owner).unwrap();
                let live_type = restored.types.type_index_of(name).unwrap();
                assert_eq!(
                    restored.store.component_bytes(current, live_type)?,
                    original
                );
            }
        } else {
            assert_eq!(
                restored.copied_scalar_values,
                if number == 4 { 157 } else { 156 }
            );
            assert_eq!(restored.cleared_references, 0);
        }
        let group = restored
            .types
            .type_index_of("Unity.Entities.LinkedEntityGroup")
            .unwrap();
        assert_eq!(restored.prefab_linked_groups, 1772);
        for source in snapshot.entities() {
            assert!(restored
                .store
                .exists(restored.bindings.target(source).unwrap())?);
        }
        for prefab in &restored.prefabs {
            for &handle in std::iter::once(&prefab.entity).chain(&prefab.linked_entities) {
                assert!(restored.store.exists(handle)?);
            }
            if !prefab.linked_entities.is_empty() {
                let bytes = restored
                    .store
                    .buffer_payload(&restored.types, prefab.entity, group)?;
                assert_eq!(bytes.len(), 8 * prefab.linked_entities.len());
                for (raw, expected) in bytes.as_chunks::<8>().0.iter().zip(&prefab.linked_entities)
                {
                    assert_eq!(read_handle(raw), *expected);
                }
            }
        }
        let mut rows = 0;
        let mut elements = 0;
        for (name, stride) in [
            ("Unity.Entities.LinkedEntityGroup", 8),
            ("ProjectM.MegaStaticDestroyedBuffer", 16),
        ] {
            let saved_type = snapshot
                .header()
                .persistent_component_named(name)
                .unwrap()
                .saved_type_index;
            let live_type = restored.types.type_index_of(name).unwrap();
            for (_, job) in snapshot.jobs() {
                for chunk in job
                    .archetypes
                    .iter()
                    .flat_map(|archetype| &archetype.chunks)
                {
                    let Some(column) = chunk
                        .columns
                        .iter()
                        .find(|column| column.saved_type_index == saved_type)
                    else {
                        continue;
                    };
                    for (source, original) in chunk.entities().zip(column.buffers().unwrap()) {
                        let target = restored.bindings.target(source).unwrap();
                        let actual =
                            restored
                                .store
                                .buffer_payload(&restored.types, target, live_type)?;
                        assert_eq!(actual.len(), original.len());
                        if live_type == group {
                            for (actual, original) in actual
                                .as_chunks::<8>()
                                .0
                                .iter()
                                .zip(original.as_chunks::<8>().0)
                            {
                                assert_eq!(
                                    read_handle(actual),
                                    restored.bindings.target(read_handle(original)).unwrap()
                                );
                            }
                        } else {
                            assert_eq!(actual, original);
                        }
                        rows += 1;
                        elements += original.len() / stride;
                    }
                }
            }
        }
        if !extended {
            assert_eq!(restored.copied_buffer_rows, rows);
            assert_eq!(restored.copied_buffer_elements, elements);
        }
        assert_eq!((rows, elements), (3153, 142));
        println!("Autosave {number}: {rows} saved buffer rows, {elements} elements");
        let assert_ref = |name, owner, offset, target| -> Result<()> {
            let component = restored.types.type_index_of(name).unwrap();
            let owner = restored.bindings.target(owner).unwrap();
            let target = restored.bindings.target(target).unwrap();
            assert!(restored.store.exists(target)?);
            let bytes = restored.store.component_bytes(owner, component)?;
            let offset: usize = offset;
            assert_eq!(bytes[offset..offset + 4], target.index.to_le_bytes());
            assert_eq!(bytes[offset + 4..offset + 8], target.version.to_le_bytes());
            Ok(())
        };
        assert_ref(
            "ProjectM.Buff",
            Entity::new(152839, 4),
            8,
            Entity::new(152830, 4),
        )?;
        if number == 4 {
            assert_ref(
                "ProjectM.EntityOwner",
                Entity::new(559509, 3),
                0,
                Entity::new(36976, 1),
            )?;
        }
    }
    Ok(())
}

fn read_handle(bytes: &[u8; 8]) -> Entity {
    Entity::new(
        i32::from_le_bytes(bytes[..4].try_into().unwrap()),
        i32::from_le_bytes(bytes[4..].try_into().unwrap()),
    )
}

#[test]
#[ignore = "requires the pinned server data and layout report"]
fn server_scene_layouts_fit_every_serialized_chunk() -> Result<()> {
    let data = local_path("VRISING_SERVER_DATA")?.join("StreamingAssets/EntityScenes");
    let layouts = TypeLayouts::from_server_json(&std::fs::read(local_path(
        "RRISING_SERVER_LAYOUT_REPORT",
    )?)?)?;
    let mut counts = [0; 4];
    for entry in std::fs::read_dir(data)? {
        let path = entry?.path();
        if path.extension().is_none_or(|ext| ext != "entities") {
            continue;
        }
        let scene = SceneFile::open(&path)?;
        let resolved = layouts
            .resolve_scene(&scene)
            .with_context(|| format!("checking {}", path.display()))?;
        assert!(
            resolved.summary.unresolved_archetypes.is_empty(),
            "{}",
            path.display()
        );
        for chunk in &resolved.chunks {
            assert_eq!(chunk.buffer.len(), 65472, "{}", path.display());
        }
        counts[0] += 1;
        counts[1] += resolved.summary.resolved_archetypes;
        counts[2] += resolved.summary.resolved_chunks;
        counts[3] += resolved.summary.resolved_entities;
    }
    println!("server files, archetypes, chunks, entities: {counts:?}");
    assert_eq!(counts, [372, 10464, 14128, 512967]);
    Ok(())
}
