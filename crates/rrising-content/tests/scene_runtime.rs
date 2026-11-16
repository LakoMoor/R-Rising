use rrising_content::{entity_scenes::SceneFile, scene_runtime::load_chunk_metadata};
use std::path::PathBuf;

#[test]
#[ignore = "requires all reference EntityScenes; set VRISING_DATA"]
fn all_original_scene_masks_survive_runtime_metadata_growth_and_swap_back() {
    let data = std::env::var_os("VRISING_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../VRising_Data"));
    let directory = data.join("StreamingAssets/EntityScenes");
    let mut paths: Vec<_> = std::fs::read_dir(&directory)
        .expect("reference scene folder")
        .map(|entry| entry.expect("scene entry").path())
        .filter(|path| path.extension().is_some_and(|s| s == "entities"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 1183);
    let mut chunks_total = 0;
    let mut entities_total = 0;
    let mut masks_total = 0;
    for path in paths {
        let scene = SceneFile::open(&path).expect("reference scene");
        let bits = scene.enabled_bits().expect("enabled bits");
        let mut runtime = load_chunk_metadata(&scene, 123).expect("runtime metadata import");
        let mut base = 0;
        for (arch, data) in runtime.iter_mut().enumerate() {
            let count = data.count();
            let components = data.component_count();
            chunks_total += count;
            for chunk in 0..count {
                entities_total +=
                    data.get_chunk_entity_count(chunk).expect("entity count") as usize;
                for component in 0..components {
                    let expected = bits.masks[base + chunk * components + component];
                    assert_eq!(
                        data.get_component_enabled_mask_array_for_type_in_chunk(component, chunk)
                            .expect("runtime mask"),
                        [expected.low, expected.high]
                    );
                    assert_eq!(
                        data.get_chunk_disabled_count_for_type(component, chunk)
                            .expect("disabled"),
                        bits.hierarchical[base + chunk * components + component]
                    );
                    masks_total += 1;
                }
            }
            if count > 0 {
                let last_masks = data
                    .get_component_enabled_mask_array_for_chunk(count - 1)
                    .expect("last masks")
                    .to_vec();
                let last_count = data.get_chunk_entity_count(count - 1).expect("last count");
                data.grow(count + 3).expect("grow imported metadata");
                if count > 1 {
                    data.remove_at_swap_back(0).expect("remove first");
                    assert_eq!(
                        data.get_component_enabled_mask_array_for_chunk(0)
                            .expect("moved masks"),
                        last_masks
                    );
                    assert_eq!(
                        data.get_chunk_entity_count(0).expect("moved count"),
                        last_count
                    );
                }
            }
            assert_eq!(components, bits.component_counts[arch]);
            base += count * components;
        }
    }
    assert_eq!(
        (chunks_total, entities_total, masks_total),
        (31723, 1385671, 648652)
    );
    println!("Imported {chunks_total} original chunks, {entities_total} entities, {masks_total} masks into runtime metadata; growth/swap-back preserved masks");
}
