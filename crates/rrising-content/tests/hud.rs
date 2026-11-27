use rrising_content::{hud::candidate_action_bar, list_archives, GameContent};
use std::path::{Path, PathBuf};

#[test]
#[ignore = "requires the pinned client installation"]
fn reads_action_bar_frame_from_original_prefab() {
    let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("VRISING_DATA"));
    let archives = list_archives(&data.join("StreamingAssets/ContentArchives")).unwrap();
    let content =
        GameContent::open(&data, &archives, Path::new("../../assets/typetrees.tpk")).unwrap();
    let hud = candidate_action_bar(&content).unwrap();
    assert_eq!(hud.count, 8);
    assert_eq!(hud.slot_size, [48., 48.]);
    assert_eq!(hud.frame_extra, [6., 6.]);
    assert_eq!(hud.anchor_position, [-103., 80.]);
    assert_eq!(hud.spacing, 8.);
    let texture = content.read_texture(&hud.texture).unwrap();
    assert_eq!((texture.width, texture.height), (132, 132));
}
