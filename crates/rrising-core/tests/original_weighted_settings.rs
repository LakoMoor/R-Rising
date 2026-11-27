#[test]
#[ignore = "requires pinned server scenes and layout report"]
#[cfg(feature = "content")]
fn installed_settings_feed_verified_selection() {
    use rrising_content::{
        entity_scenes::SceneFile, randomized_settings::read_scene_settings,
        type_layouts::TypeLayouts,
    };
    use rrising_core::weighted_spawn::select_settings;
    let data = std::path::PathBuf::from(std::env::var_os("VRISING_SERVER_DATA").unwrap());
    let layouts = TypeLayouts::from_server_json(
        &std::fs::read(std::env::var_os("RRISING_SERVER_LAYOUT_REPORT").unwrap()).unwrap(),
    )
    .unwrap();
    let mut settings = std::collections::BTreeMap::new();
    let mut rows = 0;
    for entry in std::fs::read_dir(data.join("StreamingAssets/EntityScenes")).unwrap() {
        let path = entry.unwrap().path();
        if !path.extension().is_some_and(|x| x == "entities") {
            continue;
        }
        for (guid, value) in
            read_scene_settings(&SceneFile::open(&path).unwrap(), &layouts).unwrap()
        {
            rows += 1;
            if let Some(old) = settings.insert(guid, value.clone()) {
                assert_eq!(old, value);
            }
        }
    }
    if let Some(path) = std::env::var_os("RRISING_SETTINGS_PROBE") {
        let mut probes = Vec::new();
        for (&guid, setting) in &settings {
            for seed in [0u32, 1, 99712, u32::MAX] {
                let groups:Vec<_>=setting.groups.iter().map(|g|serde_json::json!({"weight":g.weight,"total_chain_weight":g.total_chain_weight,"chains":g.chains.iter().map(|c|serde_json::json!({"guid":c.guid,"weight":c.weight})).collect::<Vec<_>>()})).collect();
                probes.push(serde_json::json!({"setting_guid":guid,"seed":seed,"groups":groups,"total_group_weight":setting.total_group_weight}));
            }
        }
        std::fs::write(path, serde_json::to_vec(&probes).unwrap()).unwrap();
    }
    assert_eq!((rows, settings.len()), (110, 55));
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evidence/oracles/weighted_spawn_original_settings.json"
    ))
    .unwrap();
    let cases = golden["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 220);
    for case in cases {
        let guid = case["setting_guid"].as_i64().unwrap() as i32;
        let mut state = case["seed"].as_u64().unwrap() as u32;
        let actual = select_settings(&settings[&guid], &mut state).unwrap();
        assert_eq!(i64::from(actual), case["guid"].as_i64().unwrap());
        assert_eq!(u64::from(state), case["state"].as_u64().unwrap());
    }
    let mut choices = 0;
    for setting in settings.values() {
        for seed in [0, 1, 99712, u32::MAX] {
            let mut state = seed;
            select_settings(setting, &mut state).unwrap();
            choices += 1;
        }
    }
    println!(
        "settings rows={rows} unique={} selections={choices}",
        settings.len()
    );
    assert!(!settings.is_empty());
}
