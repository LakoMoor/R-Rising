#[test]
#[ignore = "requires pinned server scenes and layout report"]
fn installed_chain_graphs_and_weighted_choices_resolve() {
    use rrising_content::{
        entity_scenes::SceneFile, randomized_settings::read_scene_settings,
        spawn_chain::read_scene_chains, type_layouts::TypeLayouts,
    };
    use rrising_core::weighted_spawn::select_settings;
    let data = std::path::PathBuf::from(std::env::var_os("VRISING_SERVER_DATA").unwrap());
    let layouts = TypeLayouts::from_server_json(
        &std::fs::read(std::env::var_os("RRISING_SERVER_LAYOUT_REPORT").unwrap()).unwrap(),
    )
    .unwrap();
    let mut chains = std::collections::BTreeMap::new();
    let mut settings = std::collections::BTreeMap::new();
    let mut rows = 0;
    let mut tiles = rrising_content::tile_catalog::TileTemplateCatalog::default();
    for entry in std::fs::read_dir(data.join("StreamingAssets/EntityScenes")).unwrap() {
        let path = entry.unwrap().path();
        if !path.extension().is_some_and(|x| x == "entities") {
            continue;
        }
        let scene = SceneFile::open(&path).unwrap();
        tiles.read_scene(&scene, &layouts).unwrap();
        for (guid, chain) in read_scene_chains(&scene, &layouts).unwrap() {
            rows += 1;
            if let Some(old) = chains.insert(guid, chain.clone()) {
                assert_eq!(old, chain);
            }
        }
        for (guid, value) in read_scene_settings(&scene, &layouts).unwrap() {
            settings.insert(guid, value);
        }
    }
    let elements: usize = chains.values().map(|c| c.elements.len()).sum();
    let transitions: usize = chains
        .values()
        .flat_map(|c| &c.elements)
        .map(|e| e.transitions.len())
        .sum();
    let mut resolved = 0;
    let mut tile_choices = 0;
    let mut null = 0;
    let mut missing = 0;
    for setting in settings.values() {
        for seed in [0, 1, 99712, u32::MAX] {
            let mut state = seed;
            let guid = select_settings(setting, &mut state).unwrap();
            if guid == 0 {
                null += 1;
            } else if chains.get(&guid).and_then(|c| c.main_element()).is_some() {
                resolved += 1;
                let element = chains[&guid].main_element().unwrap();
                tile_choices += usize::from(tiles.get(element.prefab_guid).is_some());
            } else {
                missing += 1;
            }
        }
    }
    println!("chain_rows={rows} unique={} elements={elements} transitions={transitions} choices_resolved={resolved} null={null} missing={missing}",chains.len());
    println!("tile_main_choices={tile_choices}");
    assert_eq!(
        (
            rows,
            chains.len(),
            elements,
            transitions,
            resolved,
            null,
            missing
        ),
        (2990, 1495, 4297, 15727, 220, 0, 0)
    );
    for chain in chains.values() {
        assert!(chain.main_element().is_some());
        for (index, element) in chain.elements.iter().enumerate() {
            assert_eq!(element.index, index as i32);
            for transition in &element.transitions {
                if !transition.end_of_chain {
                    assert!(
                        usize::try_from(transition.target).is_ok_and(|i| i < chain.elements.len())
                    );
                }
            }
        }
    }
}
