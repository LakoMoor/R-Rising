use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{ensure, Context, Result};
use rrising_content::{
    entity_scenes::SceneFile,
    persistence::{PersistentSave, SavedSnapshot},
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
        assert_eq!(
            restored.copied_scalar_values,
            if number == 4 { 157 } else { 156 }
        );
        assert_eq!(restored.cleared_references, 0);
        for source in snapshot.entities() {
            assert!(restored
                .store
                .exists(restored.bindings.target(source).unwrap())?);
        }
        for prefab in &restored.prefabs {
            for &handle in std::iter::once(&prefab.entity).chain(&prefab.linked_entities) {
                assert!(restored.store.exists(handle)?);
            }
        }
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
