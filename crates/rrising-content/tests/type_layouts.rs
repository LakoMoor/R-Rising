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
    // All 1,772 scene hashes have a storage shape: every archetype resolves and validates.
    assert_eq!((unresolved_archetypes, unresolved_chunks), (0, 0));
    assert_eq!(
        (archetypes, chunks, entities, headers, patches, transforms),
        (17579, 31723, 1385671, 274197, 89120, 41462),
        "pinned scene-layout acceptance set changed; investigate first divergence"
    );
}

fn synthetic_report(entity_size: i32, managed_size: i32) -> Vec<u8> {
    let row = |name: &str, hash: &str, size: i32, managed: bool, entity: bool| {
        format!(
            r#"{{"name":"{name}","stable_hash":"{hash}","managed_value_size":{},"ecs":{{"category":"{}","managed":{managed},"zero_sized":false,"enableable":false,"cleanup":false,"iequatable":false,"size_in_chunk":{size},"element_size":{size},"buffer_capacity":null,"alignment_in_bytes":{},"maximum_chunk_capacity":128,"memory_ordering":"{}","entity_offsets":[]}}}}"#,
            if managed { 0 } else { size },
            if entity { "entity" } else { "component" },
            if managed { 0 } else { 8 },
            if entity { "0000000000000000" } else { hash }
        )
    };
    format!(
        r#"{{"schema_version":1,"evidence_kind":"offline-derived-and-scene-hash-matched","binary_sha256":"a23f36d5b5b0e393fb77dc0680a9600016173fea6c0590bf2634f732862b0a56","metadata_sha256":"b8f9f76ffb72a0c77079082aac8eb78da839094e9d090e8461b49230e8831271","matches":[{},{}]}}"#,
        row("Unity.Entities.Entity", "6f02124d960c5b29", entity_size, false, true),
        row("Test.ManagedClass", "0123456789abcdef", managed_size, true, false)
    )
    .into_bytes()
}

#[test]
fn managed_component_admits_only_the_recovered_four_byte_slot() {
    // BuildComponentType's managed-class branch stores SizeInChunk = ElementSize = 4.
    let layouts = TypeLayouts::from_json(&synthetic_report(8, 4)).expect("4-byte managed slot");
    assert_eq!(layouts.len(), 2);
    for wrong in [0, 1, 2, 8, 16] {
        assert!(
            TypeLayouts::from_json(&synthetic_report(8, wrong)).is_err(),
            "managed slot of {wrong} bytes must stay refused"
        );
    }
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
