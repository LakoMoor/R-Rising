//! Identifies baked scene candidates; component presence does not imply a live player.
use rrising_content::{entity_scenes::SceneFile, type_layouts::TypeLayouts};
use std::{collections::BTreeMap, path::PathBuf};

#[test]
#[ignore = "requires the pinned client installation and generated layout report"]
fn inventory_player_input_and_movement_components() {
    let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("VRISING_DATA"));
    let layouts = TypeLayouts::from_json(
        &std::fs::read(std::env::var_os("RRISING_LAYOUT_REPORT").expect("RRISING_LAYOUT_REPORT"))
            .unwrap(),
    )
    .unwrap();
    let hashes = [
        0xcf49_a498_d747_4572u64,
        0x7fe9_2b06_b5b2_0fef,
        0x0a09_4831_04c7_585e,
    ];
    let mut counts = BTreeMap::<u8, usize>::new();
    let mut scenes = 0;
    for entry in std::fs::read_dir(data.join("StreamingAssets/EntityScenes")).unwrap() {
        let path = entry.unwrap().path();
        if !path.extension().is_some_and(|x| x == "entities") {
            continue;
        }
        let scene = SceneFile::open(&path).unwrap();
        let resolved = layouts.resolve_scene(&scene).unwrap();
        for chunk in &resolved.chunks {
            let arch = resolved.archetypes[chunk.header.archetype_index as usize]
                .as_ref()
                .unwrap();
            let mask = hashes.iter().enumerate().fold(0, |mask, (i, hash)| {
                mask | (u8::from(arch.hashes.contains(hash)) << i)
            });
            *counts.entry(mask).or_default() += chunk.header.count as usize;
            if mask & 4 != 0 {
                let prefab = arch.hashes.iter().any(|hash| {
                    layouts
                        .get(*hash)
                        .is_some_and(|ty| ty.name == "Unity.Entities.Prefab")
                });
                for row in 0..chunk.header.count as usize {
                    let bytes = resolved
                        .component_bytes(chunk.index, row, 0xb181_3197_8820_0ae1)
                        .unwrap()
                        .expect("player prefab guid");
                    println!(
                        "player scene={} guid={}",
                        path.display(),
                        i32::from_le_bytes(bytes.try_into().unwrap())
                    );
                }
                assert!(
                    prefab,
                    "player template without Prefab flag: {}",
                    path.display()
                );
            }
        }
        scenes += 1;
    }
    println!("client scenes={scenes}; component mask (input=1, movement=2, player=4): {counts:?}");
    assert_eq!(scenes, 1183);
    assert_eq!(counts.get(&7), Some(&2));
}
