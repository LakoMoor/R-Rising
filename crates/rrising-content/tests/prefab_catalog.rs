use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{ensure, Context, Result};
use rrising_content::{
    entity_scenes::SceneFile,
    persistence::{MegaStaticRestorePlan, PersistentSave, SavedSnapshot},
    prefab_catalog::{ScenePrefabCatalog, SERVER_PREFAB_SCENES},
    restore_world::RestoredWorldProjection,
    type_layouts::TypeLayouts,
};
use rrising_ecs::{ComponentType, Entity, EntityQuery};

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
        let disabled_name = "Unity.Entities.Disabled";
        let disabled = restored.types.type_index_of(disabled_name);
        assert_eq!(
            disabled.is_some(),
            layouts.get(0x9225_a4b0_02d7_10ea).is_some()
        );
        if let Some(disabled) = disabled {
            let saved_disabled = snapshot
                .header()
                .required_components
                .iter()
                .find(|component| component.full_name == disabled_name)
                .unwrap()
                .saved_type_index;
            assert_eq!(saved_disabled.value, 0x4002_12e8);
            assert!(snapshot
                .header()
                .persistent_component_named(disabled_name)
                .is_none());
            for (_, job) in snapshot.jobs() {
                for archetype in &job.archetypes {
                    let expected = archetype.types.contains(&saved_disabled);
                    for source in archetype.chunks.iter().flat_map(|chunk| chunk.entities()) {
                        let target = restored.bindings.target(source).unwrap();
                        assert_eq!(
                            restored
                                .store
                                .try_component_bytes(target, disabled)?
                                .is_some(),
                            expected
                        );
                    }
                }
            }
            assert_eq!(restored.restored_disabled_rows, 262167);
            assert_eq!(restored.admitted_saved_tag_types, 49);
            assert_eq!(restored.skipped_saved_tag_types, 6);
            let mut tag_counts = BTreeMap::new();
            for (_, job) in snapshot.jobs() {
                for archetype in &job.archetypes {
                    let rows: usize = archetype
                        .chunks
                        .iter()
                        .map(|chunk| chunk.entities().len())
                        .sum();
                    for ty in archetype.types.iter().filter(|ty| ty.is_zero_sized()) {
                        *tag_counts.entry(*ty).or_insert(0) += rows;
                    }
                }
            }
            let mut checked_tags = 0;
            for component in &snapshot.header().required_components {
                let Some(&expected) = tag_counts.get(&component.saved_type_index) else {
                    continue;
                };
                let Some(ty) = restored.types.type_index_of(&component.full_name) else {
                    continue;
                };
                let query = EntityQuery {
                    all: vec![ty],
                    include_disabled: true,
                    ..Default::default()
                };
                assert_eq!(
                    restored.store.query_entity_count(&query)?,
                    expected,
                    "{}",
                    component.full_name
                );
                checked_tags += 1;
            }
            assert_eq!(checked_tags, 49);
            let mut disabled_query = EntityQuery::new(&[ComponentType::read_only(disabled)]);
            assert_eq!(restored.store.query_entity_count(&disabled_query)?, 0);
            disabled_query.include_disabled = true;
            assert_eq!(restored.store.query_entity_count(&disabled_query)?, 262167);
        }
        let has_simulate_layout = layouts.get(0x2d63_24ec_1523_c9e0).is_some();
        let simulate = restored.types.type_index_of("Unity.Entities.Simulate");
        assert_eq!(simulate.is_some(), has_simulate_layout);
        if let Some(simulate) = simulate {
            let saved_simulate = snapshot
                .header()
                .required_components
                .iter()
                .find(|component| component.full_name == "Unity.Entities.Simulate")
                .unwrap()
                .saved_type_index;
            assert_eq!(saved_simulate.value, 0x4102_12eb);
            assert!(snapshot
                .header()
                .persistent_component_named("Unity.Entities.Simulate")
                .is_none());
            for (_, job) in snapshot.jobs() {
                for archetype in &job.archetypes {
                    assert!(archetype.types.contains(&saved_simulate));
                    for source in archetype.chunks.iter().flat_map(|chunk| chunk.entities()) {
                        let target = restored.bindings.target(source).unwrap();
                        assert!(restored.store.component_enabled(target, simulate)?);
                    }
                }
            }
            assert_eq!(restored.initialized_simulate_rows, snapshot.entity_count());
            if disabled.is_some() {
                let mut simulate_query = EntityQuery::new(&[ComponentType::read_only(simulate)]);
                assert_eq!(
                    restored.store.query_entity_count(&simulate_query)?,
                    snapshot.entity_count() - 262167
                );
                simulate_query.include_disabled = true;
                assert_eq!(
                    restored.store.query_entity_count(&simulate_query)?,
                    snapshot.entity_count()
                );
            }
        } else {
            assert_eq!(restored.initialized_simulate_rows, 0);
        }
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

#[test]
#[ignore = "requires the original server scenes and extended server layout report"]
fn original_prefab_groups_clone_independent_handles_and_reference_fields() -> Result<()> {
    use rrising_content::{
        prefab_catalog::{
            LinkedPrefabTemplate, PrefabMemberKey, PrefabTemplateValue, LINKED_ENTITY_GROUP_HASH,
        },
        type_layouts::ENTITY_HASH,
        world_adapter::PARENT_HASH,
    };
    use rrising_ecs::{EntityComponentStore, SpecialTypes, TypeManager, TypeSpec};

    let data = local_path("VRISING_SERVER_DATA")?.join("StreamingAssets/EntityScenes");
    let layouts = TypeLayouts::from_server_json(&std::fs::read(local_path(
        "RRISING_SERVER_LAYOUT_REPORT",
    )?)?)?;
    assert!(
        layouts.get(ENTITY_HASH).unwrap().portable_value,
        "regenerate the extended server layout report"
    );
    let mut types = TypeManager::default();
    types.register(TypeSpec::tag("<reserved null>"))?;
    let entity = types.register(layouts.get(ENTITY_HASH).unwrap().spec.clone())?;
    let prefab = types.register(layouts.get(0x616a_36e3_c0cf_ef8c).unwrap().spec.clone())?;
    let linked = types.register(layouts.get(LINKED_ENTITY_GROUP_HASH).unwrap().spec.clone())?;
    let mut store = EntityComponentStore::new(
        50000,
        SpecialTypes {
            entity_type: entity,
            prefab,
            ..Default::default()
        },
    )?;
    let mut totals = [0; 9];
    let existing = store.get_or_create_archetype(&types, &[prefab])?;
    for (scene_id, _) in SERVER_PREFAB_SCENES {
        let scene = SceneFile::open(&data.join(format!("{scene_id}.0.entities")))?;
        let topology = ScenePrefabCatalog::read(&scene, &layouts)?;
        let selected = topology
            .prefabs
            .iter()
            .map(|prefab| prefab.prefab_guid)
            .collect();
        let source = ScenePrefabCatalog::read_linked_templates(&scene, &layouts, &selected)?;
        assert_eq!(source.linked_templates.len(), selected.len());
        let sources_by_guid: BTreeMap<_, _> = topology
            .prefabs
            .iter()
            .map(|prefab| (prefab.prefab_guid, prefab))
            .collect();
        let mut external = std::collections::HashMap::new();
        for template in source.linked_templates.values() {
            for &(_, key) in template
                .members
                .iter()
                .flatten()
                .flat_map(|value| &value.external_patches)
            {
                if let std::collections::hash_map::Entry::Vacant(entry) = external.entry(key) {
                    entry.insert(store.create_entity(existing)?);
                }
            }
        }
        for (guid, template) in &source.linked_templates {
            totals[0] += 1;
            totals[1] += template.members.len();
            let original = topology
                .prefabs
                .iter()
                .find(|entry| entry.prefab_guid == *guid)
                .unwrap();
            let original_members = if original.linked_entities.is_empty() {
                std::slice::from_ref(&original.entity)
            } else {
                &original.linked_entities
            };
            assert_eq!(template.members.len(), original_members.len());
            let first =
                template.instantiate_with_external(&mut store, &mut types, &layouts, |key| {
                    external.get(&key).copied()
                })?;
            let second =
                template.instantiate_with_external(&mut store, &mut types, &layouts, |key| {
                    external.get(&key).copied()
                })?;
            let distinct: std::collections::HashSet<_> = first.iter().chain(&second).collect();
            assert_eq!(distinct.len(), first.len() + second.len());
            for group in [first, second] {
                if template.has_group {
                    let list = store.buffer_payload(&types, group[0], linked)?;
                    assert!(list
                        .as_chunks::<8>()
                        .0
                        .iter()
                        .map(read_handle)
                        .eq(group.iter().copied()));
                }
                for (values, &entity) in template.members.iter().zip(&group) {
                    assert!(store.exists(entity)?);
                    assert!(store.component_bytes(entity, prefab).is_err());
                    for value in values {
                        let ty = types
                            .infos()
                            .iter()
                            .find(|info| info.stable_type_hash == value.stable_hash)
                            .unwrap()
                            .type_index;
                        let got = if value.is_buffer {
                            store.buffer_payload(&types, entity, ty)?
                        } else {
                            store.component_bytes(entity, ty)?
                        };
                        let mut expected = value.bytes.clone();
                        for &(offset, target) in &value.reference_patches {
                            assert_eq!(
                                read_handle(expected[offset..offset + 8].try_into()?),
                                original_members[target]
                            );
                            expected[offset..offset + 4]
                                .copy_from_slice(&group[target].index.to_le_bytes());
                            expected[offset + 4..offset + 8]
                                .copy_from_slice(&group[target].version.to_le_bytes());
                            totals[3] += 1;
                            if value.is_buffer {
                                totals[4] += 1;
                            }
                        }
                        for &(offset, key) in &value.external_patches {
                            let target = external[&key];
                            assert_eq!(
                                read_handle(expected[offset..offset + 8].try_into()?),
                                key.resolve(sources_by_guid[&key.guid]).unwrap()
                            );
                            assert_ne!(key.guid, *guid);
                            assert!(!group.contains(&target));
                            assert!(store.exists(target)?);
                            expected[offset..offset + 4]
                                .copy_from_slice(&target.index.to_le_bytes());
                            expected[offset + 4..offset + 8]
                                .copy_from_slice(&target.version.to_le_bytes());
                            totals[5] += 1;
                        }
                        assert_eq!(
                            got,
                            expected,
                            "{guid} / {}",
                            layouts.get(value.stable_hash).unwrap().name
                        );
                        if let Some(enabled) = value.enabled {
                            assert_eq!(store.component_enabled(entity, ty)?, enabled);
                            totals[7 + usize::from(enabled)] += 1;
                        }
                        if value.bytes.is_empty() && !value.is_buffer {
                            totals[6] += 1;
                        }
                        totals[2] += 1;
                    }
                }
            }
        }
    }
    println!("prefabs, original members, cloned values, remapped fields, remapped buffer fields, external fields, tags, disabled, enabled: {totals:?}");
    assert_eq!(totals[..2], [23583, 41205]);
    let key = PrefabMemberKey {
        guid: 123,
        ordinal: 1,
    };
    let target = store.create_entity(existing)?;
    let template = LinkedPrefabTemplate {
        members: vec![vec![PrefabTemplateValue {
            stable_hash: PARENT_HASH,
            is_buffer: false,
            enabled: None,
            bytes: vec![0; 8],
            reference_patches: Vec::new(),
            external_patches: vec![(0, key)],
        }]],
        has_group: false,
    };
    let resolved =
        template.instantiate_with_external(&mut store, &mut types, &layouts, |found| {
            assert_eq!(found, key);
            Some(target)
        })?;
    let parent = types.type_index_of("Unity.Transforms.Parent").unwrap();
    assert_eq!(
        read_handle(store.component_bytes(resolved[0], parent)?.try_into()?),
        target
    );
    let unresolved = template.instantiate(&mut store, &mut types, &layouts)?;
    assert!(store.component_bytes(unresolved[0], parent).is_err());
    store.destroy_entity(target)?;
    assert!(template
        .instantiate_with_external(&mut store, &mut types, &layouts, |_| Some(target))
        .is_err());
    Ok(())
}

#[test]
#[ignore = "requires a captured original save and server layout report"]
fn supplemental_server_time_matches_saved_rows() -> Result<()> {
    let data = local_path("VRISING_SERVER_DATA")?.join("StreamingAssets/EntityScenes");
    let layouts = TypeLayouts::from_server_json(&std::fs::read(local_path(
        "RRISING_SERVER_LAYOUT_REPORT",
    )?)?)?;
    let save = PersistentSave::open(&local_path("RRISING_SAVE_FILE")?)?;
    let snapshot = SavedSnapshot::read(&save)?;
    let mut catalogs = Vec::new();
    for (scene_id, _) in SERVER_PREFAB_SCENES {
        let scene = SceneFile::open(&data.join(format!("{scene_id}.0.entities")))?;
        catalogs.push(ScenePrefabCatalog::read(&scene, &layouts)?);
    }
    let restored = RestoredWorldProjection::build(&snapshot, &layouts, &catalogs)?;
    let saved = snapshot
        .header()
        .persistent_component_named("ProjectM.ServerTime")
        .context("saved ServerTime metadata")?;
    let layout = layouts
        .get(saved.stable_type_hash)
        .context("ServerTime layout")?;
    ensure!(
        layout.entity_offsets.is_empty(),
        "ServerTime contains entity references"
    );
    let live = restored
        .types
        .type_index_of("ProjectM.ServerTime")
        .context("restored ServerTime")?;
    let mut checked = 0;
    for (_, job) in snapshot.jobs() {
        for archetype in &job.archetypes {
            for chunk in &archetype.chunks {
                for column in chunk
                    .columns
                    .iter()
                    .filter(|column| column.saved_type_index == saved.saved_type_index)
                {
                    for (index, entity) in chunk.entities().enumerate() {
                        let target = restored
                            .bindings
                            .target(entity)
                            .context("saved entity binding")?;
                        assert_eq!(
                            restored.store.component_bytes(target, live)?,
                            column.scalar(index).context("ServerTime scalar")?
                        );
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 0);
    Ok(())
}

#[test]
#[ignore = "requires the original server prefab scenes"]
fn server_prefab_scene_blob_offsets_resolve() -> Result<()> {
    let data = local_path("VRISING_SERVER_DATA")?.join("StreamingAssets/EntityScenes");
    let mut total = 0;
    let mut payload_bytes = 0;
    for (scene_id, _) in SERVER_PREFAB_SCENES {
        let scene = SceneFile::open(&data.join(format!("{scene_id}.0.entities")))?;
        let blobs = scene.blob_assets()?;
        for blob in &blobs.blobs {
            let offset = i32::try_from(blob.offset)?;
            let found = blobs.find_by_offset(offset).context("blob offset lookup")?;
            assert_eq!((found.hash, found.data), (blob.hash, blob.data));
            total += 1;
            payload_bytes += blob.data.len();
        }
    }
    println!("server prefab scene blobs={total} payload bytes={payload_bytes}");
    assert!(total > 0);
    Ok(())
}

#[test]
#[ignore = "requires original server scenes, layout report, and captured save"]
fn saved_blob_sources_match_scene_prefab_components() -> Result<()> {
    use std::collections::HashMap;
    let data = local_path("VRISING_SERVER_DATA")?.join("StreamingAssets/EntityScenes");
    let layouts = TypeLayouts::from_server_json(&std::fs::read(local_path(
        "RRISING_SERVER_LAYOUT_REPORT",
    )?)?)?;
    let save = PersistentSave::open(&local_path("RRISING_SAVE_FILE")?)?;
    let header = save.header()?;
    let saved: HashMap<_, _> = header
        .prefabs
        .iter()
        .map(|prefab| (prefab.prefab_guid, prefab))
        .collect();
    let mut sources = HashMap::new();
    for blob in &header.blobs {
        sources
            .entry(blob.entity)
            .or_insert_with(Vec::new)
            .push(blob);
    }
    let mut found = 0;
    let mut nonnull = 0;
    let mut missing = 0;
    for (scene_id, _) in SERVER_PREFAB_SCENES {
        let scene = SceneFile::open(&data.join(format!("{scene_id}.0.entities")))?;
        let catalog = ScenePrefabCatalog::read(&scene, &layouts)?;
        let resolved = layouts.resolve_scene(&scene)?;
        let buffers = scene.buffer_data()?;
        let blobs = scene.blob_assets()?;
        let mut locations = HashMap::new();
        for (ci, chunk) in resolved.chunks.iter().enumerate() {
            for row in 0..usize::try_from(chunk.header.count)? {
                if let Some(bytes) =
                    resolved.component_bytes(ci, row, rrising_content::type_layouts::ENTITY_HASH)?
                {
                    let entity = Entity::new(
                        i32::from_le_bytes(bytes[..4].try_into()?),
                        i32::from_le_bytes(bytes[4..8].try_into()?),
                    );
                    locations.insert(entity, (ci, row));
                }
            }
        }
        for prefab in &catalog.prefabs {
            let Some(old) = saved.get(&prefab.prefab_guid) else {
                continue;
            };
            let members = std::iter::once((old.entity, prefab.entity)).chain(
                old.linked_entities
                    .iter()
                    .copied()
                    .zip(prefab.linked_entities.iter().copied()),
            );
            for (source, current) in members {
                let Some(records) = sources.remove(&source) else {
                    continue;
                };
                let &(ci, row) = locations.get(&current).context("missing prefab member")?;
                for record in records {
                    let Some(layout) = layouts.get(record.component_stable_type_hash) else {
                        missing += 1;
                        continue;
                    };
                    let bytes = if layout.spec.category == rrising_ecs::TypeCategory::BufferData {
                        resolved
                            .buffer_payload(
                                &buffers,
                                ci,
                                row,
                                record.component_stable_type_hash,
                                usize::try_from(layout.spec.element_size)?,
                            )?
                            .and_then(|payload| {
                                let start = usize::try_from(record.element_index)
                                    .ok()?
                                    .checked_mul(layout.spec.element_size as usize)?;
                                payload.get(start..start + layout.spec.element_size as usize)
                            })
                    } else {
                        resolved.component_bytes(ci, row, record.component_stable_type_hash)?
                    };
                    let Some(value) = bytes.and_then(|bytes| {
                        usize::try_from(record.blob_field_offset)
                            .ok()
                            .and_then(|offset| bytes.get(offset..offset + 4))
                    }) else {
                        missing += 1;
                        continue;
                    };
                    let offset = i32::from_le_bytes(value.try_into()?);
                    if offset == -1 || blobs.find_by_offset(offset).is_some() {
                        found += 1;
                        nonnull += usize::from(offset != -1);
                    } else {
                        missing += 1;
                    }
                }
            }
        }
    }
    println!(
        "source fields resolved={found} nonnull={nonnull} missing={missing} unbound={}",
        sources.values().map(Vec::len).sum::<usize>()
    );
    assert_eq!((found, nonnull, missing), (4932, 2930, 1));
    assert!(sources.is_empty());
    Ok(())
}
