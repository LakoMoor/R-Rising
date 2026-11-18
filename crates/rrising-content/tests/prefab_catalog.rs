use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{ensure, Context, Result};
use rrising_content::{
    entity_scenes::SceneFile, persistence::PersistentSave, prefab_catalog::ScenePrefabCatalog,
    type_layouts::TypeLayouts,
};

const SCENES: [(&str, usize); 9] = [
    ("ad37c0edd53365b40acb48df81e07046", 2023),
    ("8137c0edd53365b40acb48df81e07046", 1189),
    ("153b723300728974d8e2bad70c3fd1f2", 3687),
    ("3429e7b0ff91dfa4db4318bcdb0e63bc", 1558),
    ("4361b3a547128724892dae4688ec7146", 395),
    ("b2f35358da120c04c92b9256f40d76ce", 4842),
    ("295c92b7a799a3443b40e9ef192b5727", 8548),
    ("5e3be03ad2064bc4a9644c4a1a30af42", 364),
    ("de0ffb819455f8f4c8eea0748b1012b6", 977),
];

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
    for (scene_id, expected_count) in SCENES {
        let scene = SceneFile::open(&data.join(format!("{scene_id}.0.entities")))?;
        let roots = ScenePrefabCatalog::read(&scene, &layouts)
            .with_context(|| format!("building catalog for {scene_id}"))?;
        assert_eq!(
            (roots.prefabs.len(), roots.unresolved_rows),
            (expected_count, 0),
            "{scene_id}"
        );
        for root in roots.prefabs {
            ensure!(
                catalog
                    .insert(root.prefab_guid, root.linked_entities.len())
                    .is_none(),
                "repeated registered GUID {}",
                root.prefab_guid
            );
        }
    }
    assert_eq!(catalog.len(), 23583);
    for number in [4, 5] {
        let header =
            PersistentSave::open(&saves.join(format!("AutoSave_{number}.save.gz")))?.header()?;
        let expected: BTreeMap<_, _> = header
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
