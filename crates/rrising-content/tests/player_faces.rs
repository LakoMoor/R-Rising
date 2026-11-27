//! Tests on the owner's pinned installation; no game asset bytes are stored here.
use rrising_content::{list_archives, player_customization::candidate_male_faces, GameContent};
use std::path::{Path, PathBuf};

#[test]
#[ignore = "requires the pinned client installation"]
fn player_face_collection_resolves_all_entries() {
    let data = PathBuf::from(std::env::var_os("VRISING_DATA").expect("VRISING_DATA"));
    let archives = list_archives(&data.join("StreamingAssets/ContentArchives")).unwrap();
    let content =
        GameContent::open(&data, &archives, Path::new("../../assets/typetrees.tpk")).unwrap();
    let faces = candidate_male_faces(&content).unwrap();
    assert_eq!(faces.len(), 15);
    let names: Vec<_> = faces
        .iter()
        .map(|(prefab, unknown)| {
            assert_eq!(*unknown, 0);
            let value = content.player().decode(prefab).unwrap();
            value
                .get("m_Name")
                .and_then(|name| name.as_str())
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(names[0], "VampireMale_Face_Standard_Prefab");
    assert_eq!(names[14], "VampireMale_Face15_Incubus_Prefab");
    assert_eq!(
        names
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        15
    );
    println!("resolved {} face prefab entries: {names:?}", faces.len());
}
