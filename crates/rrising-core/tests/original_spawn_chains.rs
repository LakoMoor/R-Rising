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
    let mut automatic = 0;
    let mut reachable = 0;
    let mut unreachable = 0;
    for chain in chains.values() {
        assert!(chain.main_element().is_some());
        for (index, element) in chain.elements.iter().enumerate() {
            assert_eq!(element.index, index as i32);
            if rrising_core::auto_chain::definition_duration_until_main(chain, index as i32)
                .is_some()
            {
                reachable += 1;
            } else {
                unreachable += 1;
            }
            if element.use_auto_transition {
                automatic += 1;
                let config = &element.auto_transition;
                assert!(config.duration.is_finite() && config.attempt_cooldown.is_finite());
                assert!(usize::try_from(config.transition_index)
                    .is_ok_and(|i| i < element.transitions.len()));
            }
            for transition in &element.transitions {
                if !transition.end_of_chain {
                    assert!(
                        usize::try_from(transition.target).is_ok_and(|i| i < chain.elements.len())
                    );
                }
            }
        }
    }
    println!(
        "automatic_elements={automatic} auto_paths_reachable={reachable} failed={unreachable}"
    );
    let started = std::time::Instant::now();
    let mut selected = 0;
    let mut unsupported_terminal_ids = 0;
    for chain in chains.values() {
        for (index, element) in chain.elements.iter().enumerate() {
            let ids: std::collections::BTreeSet<_> =
                element.transitions.iter().map(|t| t.id).collect();
            for id in ids {
                if element.transitions.iter().any(|t| {
                    t.id == id
                        && usize::try_from(t.target).map_or(true, |i| i >= chain.elements.len())
                }) {
                    unsupported_terminal_ids += 1;
                    continue;
                }
                let result = rrising_core::transition_selection::select(
                    chain,
                    &rrising_core::transition_selection::SelectionRequest {
                        transition_id: id,
                        start_element: index as i32,
                        loop_on_end: false,
                        forwarded_seconds: 0.,
                        local_to_world: [0.; 16],
                        last_failed_importance: None,
                    },
                    // Supplied lookup: this measures definition execution, not prefab availability.
                    |_| Some(rrising_ecs::Entity::new(1, 1)),
                )
                .unwrap();
                if let Some(candidate) = result.candidate {
                    assert!(usize::try_from(candidate.element_index)
                        .is_ok_and(|i| i < chain.elements.len()));
                    selected += 1;
                }
            }
        }
    }
    println!("selection_real_definition_requests={selected} unsupported_terminal_ids={unsupported_terminal_ids} rust_only_us={}",started.elapsed().as_micros());
    assert_eq!((selected, unsupported_terminal_ids), (11893, 0));
}
