//! Requires a local probe report and the exact original install; not a portable CI oracle.
use rrising_content::{
    entity_scenes::SceneFile,
    type_layouts::{TypeLayouts, LOCAL_TO_WORLD_HASH, LOCAL_TRANSFORM_HASH, TRANSLATION_HASH},
};
use std::path::PathBuf;

#[test]
#[ignore = "requires the pinned original installation and RRISING_LAYOUT_REPORT"]
fn recovered_layouts_validate_real_chunk_headers_and_buffers() {
    let report = std::env::var_os("RRISING_LAYOUT_REPORT").expect("set RRISING_LAYOUT_REPORT");
    let data = std::env::var_os("VRISING_DATA")
        .map(PathBuf::from)
        .expect("set VRISING_DATA");
    let layouts =
        TypeLayouts::from_json(&std::fs::read(report).expect("read local type-layout report"))
            .expect("layout report");
    let mut paths: Vec<_> = std::fs::read_dir(data.join("StreamingAssets/EntityScenes"))
        .expect("scenes")
        .map(|p| p.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "entities"))
        .collect();
    paths.sort();
    let mut material_mesh_records = 0;
    let (
        mut scenes,
        mut archetypes,
        mut chunks,
        mut entities,
        mut headers,
        mut patches,
        mut transforms,
    ) = (0, 0, 0, 0, 0, 0, 0);
    let (mut unresolved_archetypes, mut unresolved_chunks) = (0, 0);
    for path in paths {
        let scene = SceneFile::open(&path).expect("read original scene");
        let resolved = layouts
            .resolve_scene(&scene)
            .unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        scenes += 1;
        unresolved_archetypes += resolved.summary.unresolved_archetypes.len();
        unresolved_chunks += resolved.summary.unresolved_chunks;
        archetypes += resolved.summary.resolved_archetypes;
        chunks += resolved.summary.resolved_chunks;
        entities += resolved.summary.resolved_entities;
        headers += resolved.summary.buffer_headers;
        patches += resolved.summary.external_buffer_patches;
        for (index, chunk) in resolved.chunks.iter().enumerate() {
            if chunk.header.count == 0 {
                continue;
            }
            for entity in 0..chunk.header.count as usize {
                if let Some(bytes) = resolved
                    .component_bytes(index, entity, 0x3ae3_a524_8a68_9306)
                    .expect("MaterialMeshInfo range")
                {
                    let info =
                        rrising_ecs::MaterialMeshInfo::from_bytes(bytes).expect("MaterialMeshInfo");
                    if !info.is_runtime_mesh() && !info.has_material_mesh_index_range() {
                        assert!(info.mesh_array_index() >= 0);
                    }
                    material_mesh_records += 1;
                }
            }
            if resolved
                .component_f32s::<3>(index, 0, TRANSLATION_HASH)
                .expect("Translation byte range")
                .is_some()
            {
                transforms += 1;
            }
            if resolved
                .component_f32s::<8>(index, 0, LOCAL_TRANSFORM_HASH)
                .expect("LocalTransform byte range")
                .is_some()
            {
                transforms += 1;
            }
            if resolved
                .component_f32s::<16>(index, 0, LOCAL_TO_WORLD_HASH)
                .expect("LocalToWorld byte range")
                .is_some()
            {
                transforms += 1;
            }
        }
    }
    println!("recovered scene layouts: scenes={scenes} archetypes={archetypes} chunks={chunks} entities={entities} buffer_headers={headers} external_patches={patches} transform_chunks={transforms} unresolved_archetypes={unresolved_archetypes} unresolved_chunks={unresolved_chunks}");
    println!("MaterialMeshInfo records read: {material_mesh_records}");
    assert_eq!(scenes, 1183);
    // Remaining gaps need 64 identity-only hashes (61 managed classes, MountModelStreaming, 2 generics).
    assert_eq!((unresolved_archetypes, unresolved_chunks), (7539, 7895));
    assert_eq!(
        (archetypes, chunks, entities, headers, patches, transforms),
        (10040, 23828, 1337840, 58788, 15995, 27597),
        "pinned scene-layout acceptance set changed; investigate first divergence"
    );
}

#[test]
fn rejects_wrong_build_or_unknown_schema_before_layout_construction() {
    for bytes in [
        br#"{}"#.as_slice(),
        br#"{"schema_version":2}"#.as_slice(),
        br#"{"schema_version":1,"evidence_kind":"guessed"}"#.as_slice(),
        br#"{"schema_version":1,"evidence_kind":"offline-derived-and-scene-hash-matched","binary_sha256":"other","metadata_sha256":"other","matches":[]}"#.as_slice(),
    ] {
        assert!(TypeLayouts::from_json(bytes).is_err());
    }
}
