use rrising_ecs::{Entity, Hash128};
use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context, Result};
use rrising_content::{
    persistence::{MegaStaticRestorePlan, PersistentSave, SavedSnapshot},
    prefab_catalog::ScenePrefabCatalog,
    restore_world::RestoredWorldProjection,
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
    let data = env_path("VRISING_SERVER_DATA")?;
    let catalog = StaticTransformCatalog::read_server(&data, &layouts)?;
    let prefabs = ScenePrefabCatalog::read_server_game_data(&data, &layouts)?;
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
        let restored = RestoredWorldProjection::build_with_terrain(
            &snapshot, &layouts, &prefabs, &plan, &catalog,
        )?;
        assert_eq!(restored.placed_mega_statics, 235589);
        assert_eq!(
            restored.bindings.len() - 1,
            if number == 4 { 551032 } else { 551030 }
        );
        assert_eq!(restored.cleared_references, 12);
        assert_eq!(restored.missing_references.stale_version, 12);
        assert_eq!(restored.missing_references.nonpersistent, 0);
        assert_eq!(restored.missing_references.distinct_handles, 6);
        let type_of = |name| restored.types.type_index_of(name).unwrap();
        let translation = type_of("Unity.Transforms.Translation");
        let rotation = type_of("Unity.Transforms.Rotation");
        let static_index = type_of("ProjectM.StaticTransformIndex");
        let instance_type = type_of("ProjectM.MegaStaticInstanceBuffer");
        let prefab_guid = type_of("Stunlock.Core.PrefabGUID");
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
            let result = catalog
                .placed(entry.terrain_chunk, entry.transform_index)
                .unwrap();
            let [x, z] = entry
                .terrain_chunk
                .to_le_bytes()
                .map(|axis| f32::from(axis as i8));
            let [local_x, height, local_z] = transform.position;
            let [turned_x, turned_z] = match source.rotation_degrees {
                0 => [local_x, local_z],
                90 => [local_z, 160.0 - local_x],
                180 => [160.0 - local_x, 160.0 - local_z],
                270 => [160.0 - local_z, local_x],
                _ => panic!("unexpected world terrain rotation"),
            };
            let expected = [
                x * 160.0 - 3200.0 + turned_x,
                height,
                z * 160.0 - 3200.0 + turned_z,
            ];
            for (actual, expected) in result.position.into_iter().zip(expected) {
                assert!((actual - expected).abs() < 0.001);
            }
            let (sin, cos) = ((source.rotation_degrees as f32).to_radians() * 0.5).sin_cos();
            let [qx, qy, qz, qw] = transform.rotation;
            let expected_rotation = [
                cos * qx + sin * qz,
                cos * qy + sin * qw,
                cos * qz - sin * qx,
                cos * qw - sin * qy,
            ];
            let dot: f32 = result
                .rotation
                .into_iter()
                .zip(expected_rotation)
                .map(|(actual, expected)| actual * expected)
                .sum();
            assert!((1.0 - dot.abs()).abs() < 1e-5);
            let entity = restored.bindings.target(entry.saved_instance).unwrap();
            assert!(restored.store.exists(entity)?);
            for (ty, packed) in [
                (
                    static_index,
                    u32::from(entry.terrain_chunk) | (u32::from(entry.transform_index) << 16),
                ),
                (prefab_guid, entry.prefab_guid as u32),
            ] {
                assert_eq!(
                    restored.store.component_bytes(entity, ty)?,
                    packed.to_le_bytes()
                );
            }
            assert_eq!(
                restored.store.component_bytes(entity, translation)?,
                result.position.map(f32::to_le_bytes).as_flattened()
            );
            assert_eq!(
                restored.store.component_bytes(entity, rotation)?,
                result.rotation.map(f32::to_le_bytes).as_flattened()
            );
            let manager = restored.bindings.target(entry.saved_manager).unwrap();
            let buffer = restored
                .store
                .buffer_payload(&restored.types, manager, instance_type)?;
            let index = entry.buffer_ordinal * 12;
            let current = Entity::new(
                i32::from_le_bytes(buffer[index..index + 4].try_into()?),
                i32::from_le_bytes(buffer[index + 4..index + 8].try_into()?),
            );
            assert_eq!(current, entity);
            *rotations.entry(source.rotation_degrees).or_insert(0) += 1;
        }
        assert_eq!(
            rotations,
            BTreeMap::from([(0, 143043), (90, 23792), (180, 22124), (270, 46630)])
        );
    }
    Ok(())
}
