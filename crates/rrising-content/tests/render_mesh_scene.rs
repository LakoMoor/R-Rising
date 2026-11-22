//! Offline inventory of MaterialMeshInfo modes in the pinned original scenes.
use rrising_content::{entity_scenes::SceneFile, type_layouts::TypeLayouts};
use rrising_ecs::MaterialMeshInfo;
use std::path::PathBuf;

#[test]
#[ignore = "requires pinned original scenes and RRISING_LAYOUT_REPORT"]
fn original_scene_material_mesh_modes() {
    let report = std::env::var_os("RRISING_LAYOUT_REPORT").expect("layout report");
    let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("game data"));
    let layouts = TypeLayouts::from_json(&std::fs::read(report).unwrap()).unwrap();
    let mut totals = [0usize; 4];
    for path in std::fs::read_dir(data.join("StreamingAssets/EntityScenes")).unwrap() {
        let path = path.unwrap().path();
        if !path.extension().is_some_and(|ext| ext == "entities") {
            continue;
        }
        let scene = SceneFile::open(&path).unwrap();
        let resolved = layouts.resolve_scene(&scene).unwrap();
        for (chunk_index, chunk) in resolved.chunks.iter().enumerate() {
            for row in 0..chunk.header.count as usize {
                let Some(bytes) = resolved
                    .component_bytes(chunk_index, row, 0x3ae3_a524_8a68_9306)
                    .unwrap()
                else {
                    continue;
                };
                let info = MaterialMeshInfo::from_bytes(bytes).unwrap();
                let mode = if info.has_material_mesh_index_range() {
                    0
                } else if info.is_runtime_mesh() {
                    1
                } else if info.is_runtime_material() {
                    2
                } else {
                    3
                };
                totals[mode] += 1;
            }
        }
    }
    println!("range, runtime mesh, runtime material, static: {totals:?}");
    assert_eq!(totals, [0, 0, 0, 809_820]);
}
