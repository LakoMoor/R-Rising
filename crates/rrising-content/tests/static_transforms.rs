use rrising_ecs::Hash128;
use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, Result};
use rrising_content::{
    persistence::{MegaStaticRestorePlan, PersistentSave, SavedSnapshot},
    static_transforms::StaticTransformCatalog,
    type_layouts::TypeLayouts,
};

fn env_path(name: &str) -> Result<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .with_context(|| format!("set {name}"))
}

#[test]
#[ignore = "requires pinned server scenes, server layout report and captured autosaves"]
fn original_static_transform_rows() -> Result<()> {
    let layouts =
        TypeLayouts::from_server_json(&std::fs::read(env_path("RRISING_SERVER_LAYOUT_REPORT")?)?)?;
    let catalog = StaticTransformCatalog::read_server(&env_path("VRISING_SERVER_DATA")?, &layouts)?;
    assert_eq!(
        (
            catalog.chunk_count(),
            catalog.template_count(),
            catalog.transform_count()
        ),
        (248, 189, 227658)
    );
    let anchor = catalog.source(2).context("terrain chunk (2,0)")?;
    assert_eq!(
        anchor.scene,
        Hash128::from_guid("672ca7a93fee20b46a9c7d87ff39a4c8")
    );
    assert_eq!(anchor.rotation_degrees, 0);
    let anchor_transform = catalog.get(2, 3).context("anchor transform")?;
    assert_eq!(
        anchor_transform.position.map(f32::to_bits),
        [96.75f32, 0.0, 109.25].map(f32::to_bits)
    );
    assert_eq!(
        anchor_transform.rotation.map(f32::to_bits),
        [0, 0x3f800000, 0, 0xb33bbd2e]
    );
    assert!(catalog.get(2, u16::MAX).is_none());
    for number in [4, 5] {
        let save = PersistentSave::open(
            &env_path("RRISING_SAVE_DIR")?.join(format!("AutoSave_{number}.save.gz")),
        )?;
        let snapshot = SavedSnapshot::read(&save)?;
        let plan = MegaStaticRestorePlan::read(&snapshot)?;
        let mut rotations = BTreeMap::new();
        assert_eq!(plan.entries.len(), 235589);
        for entry in &plan.entries {
            let transform = catalog
                .get(entry.terrain_chunk, entry.transform_index)
                .with_context(|| format!("missing baked transform for {entry:?}"))?;
            assert!(transform
                .position
                .iter()
                .chain(&transform.rotation)
                .all(|value| value.is_finite()));
            let source = catalog.source(entry.terrain_chunk).unwrap();
            *rotations.entry(source.rotation_degrees).or_insert(0) += 1;
        }
        assert_eq!(
            rotations,
            BTreeMap::from([(0, 143043), (90, 23792), (180, 22124), (270, 46630)])
        );
    }
    Ok(())
}
