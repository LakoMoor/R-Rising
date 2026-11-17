use std::collections::{HashMap, HashSet};

use rrising_content::persistence::{
    PersistentSave, PrefabReference, SavedEntityBindings, SavedSnapshot,
};
use rrising_ecs::Entity;
use serde_json::Value;

fn number(row: &Value, key: &str) -> usize {
    usize::try_from(row[key].as_u64().unwrap_or_else(|| panic!("{key}"))).expect(key)
}

fn check_prefab_topology(prefabs: &[PrefabReference], expected: &Value) -> HashSet<Entity> {
    let guids: HashSet<_> = prefabs.iter().map(|prefab| prefab.prefab_guid).collect();
    let roots: HashSet<_> = prefabs.iter().map(|prefab| prefab.entity).collect();
    let linked: HashSet<_> = prefabs
        .iter()
        .flat_map(|prefab| prefab.linked_entities.iter().copied())
        .collect();
    let mut source_guids = HashMap::new();
    let mut multi_guid_sources = HashSet::new();
    for prefab in prefabs {
        for source in std::iter::once(prefab.entity).chain(prefab.linked_entities.iter().copied()) {
            if *source_guids.entry(source).or_insert(prefab.prefab_guid) != prefab.prefab_guid {
                multi_guid_sources.insert(source);
            }
        }
    }
    let group_sizes: Vec<_> = prefabs
        .iter()
        .filter_map(|prefab| {
            (!prefab.linked_entities.is_empty()).then_some(prefab.linked_entities.len())
        })
        .collect();
    for (key, actual) in [
        ("prefab_entries", prefabs.len()),
        ("unique_guids", guids.len()),
        ("duplicate_guid_entries", prefabs.len() - guids.len()),
        (
            "negative_guid_entries",
            prefabs
                .iter()
                .filter(|prefab| prefab.prefab_guid < 0)
                .count(),
        ),
        (
            "zero_guid_entries",
            prefabs
                .iter()
                .filter(|prefab| prefab.prefab_guid == 0)
                .count(),
        ),
        ("unique_roots", roots.len()),
        ("linked_entries", group_sizes.iter().sum()),
        ("unique_linked_entities", linked.len()),
        ("prefabs_without_a_group", prefabs.len() - group_sizes.len()),
        ("nonempty_groups", group_sizes.len()),
        (
            "min_nonempty_group_size",
            *group_sizes.iter().min().expect("measured nonempty group"),
        ),
        (
            "max_group_size",
            *group_sizes.iter().max().expect("measured nonempty group"),
        ),
        (
            "own_root_in_first_linked_position",
            prefabs
                .iter()
                .filter(|prefab| prefab.linked_entities.first() == Some(&prefab.entity))
                .count(),
        ),
        ("roots_also_linked", roots.intersection(&linked).count()),
        ("source_handles_in_multiple_guids", multi_guid_sources.len()),
        ("unique_source_handles", source_guids.len()),
        (
            "null_source_handles",
            usize::from(source_guids.contains_key(&Entity::NULL)),
        ),
    ] {
        assert_eq!(actual, number(expected, key), "{key}");
    }
    source_guids.into_keys().collect()
}

fn fresh_job_entity(ordinal: usize) -> Entity {
    let ordinal = i32::try_from(ordinal).expect("job entity ordinal");
    Entity::new(1_000_000 + ordinal, 17 + ordinal % 3)
}

fn fresh_prefab_entity(entity: Entity) -> Entity {
    Entity::new(2_000_000 - entity.index, 5 + entity.version)
}

fn fnv1a64_in_wire_order(entities: &[Entity]) -> String {
    let hash = entities.iter().fold(0xcbf29ce484222325u64, |hash, entity| {
        entity
            .index
            .to_le_bytes()
            .into_iter()
            .chain(entity.version.to_le_bytes())
            .fold(hash, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            })
    });
    format!("{hash:016x}")
}

fn check_reference_patch(
    snapshot: &SavedSnapshot<'_>,
    bindings: &SavedEntityBindings,
    name: &str,
    owner: Entity,
    offset: usize,
    source: Entity,
) {
    let component = snapshot.header().persistent_component_named(name).unwrap();
    let stride = usize::try_from(component.component_size).unwrap();
    let owner = snapshot.entity(owner).unwrap();
    let bytes = if component.saved_type_index.is_buffer() {
        owner.buffer(component.saved_type_index).unwrap()
    } else {
        owner.scalar(component.saved_type_index).unwrap()
    };
    let mut element = bytes[..stride].to_vec();
    let mut original = Vec::from(source.index.to_le_bytes());
    original.extend(source.version.to_le_bytes());
    assert_eq!(&element[offset..offset + 8], original);
    assert_eq!(
        bindings
            .patch_references(&mut element, stride, &[offset])
            .unwrap(),
        0
    );
    let target = bindings.target(source).unwrap();
    assert_eq!(element[offset..offset + 4], target.index.to_le_bytes());
    assert_eq!(
        element[offset + 4..offset + 8],
        target.version.to_le_bytes()
    );
    assert_eq!(element[..offset], bytes[..offset]);
    assert_eq!(element[offset + 8..], bytes[offset + 8..stride]);
}

#[test]
#[ignore = "requires two captured original server saves and RRISING_SAVE_DIR"]
fn original_source_topology_binds_in_wire_order_to_supplied_synthetic_targets() {
    let root =
        std::path::PathBuf::from(std::env::var_os("RRISING_SAVE_DIR").expect("RRISING_SAVE_DIR"));
    let golden: Value =
        serde_json::from_str(include_str!("../../../evidence/oracles/save_bindings.json"))
            .expect("captured source topology");
    for capture in golden["saves"].as_array().expect("captures") {
        let name = capture["file"].as_str().expect("save file");
        let save = PersistentSave::open(&root.join(name)).expect("original autosave");
        let snapshot = SavedSnapshot::read(&save).expect("original snapshot");
        let prefabs = &snapshot.header().prefabs;
        let prefab_sources = check_prefab_topology(prefabs, &golden["prefab_topology"]);
        let job_sources: Vec<_> = snapshot
            .jobs()
            .iter()
            .flat_map(|(_, job)| &job.archetypes)
            .flat_map(|archetype| &archetype.chunks)
            .flat_map(|chunk| chunk.entities())
            .collect();
        assert_eq!(job_sources.len(), number(capture, "job_entities"), "{name}");
        assert_eq!(
            job_sources
                .iter()
                .filter(|entity| prefab_sources.contains(entity))
                .count(),
            number(capture, "job_prefab_overlap"),
            "{name}"
        );
        assert_eq!(
            fnv1a64_in_wire_order(&job_sources),
            capture["job_entity_order_fnv1a64"]
                .as_str()
                .expect("ordered original entity hash"),
            "{name}"
        );
        let synthetic_prefabs: Vec<_> = prefabs
            .iter()
            .rev()
            .map(|prefab| PrefabReference {
                prefab_guid: prefab.prefab_guid,
                entity: fresh_prefab_entity(prefab.entity),
                linked_entities: prefab
                    .linked_entities
                    .iter()
                    .copied()
                    .map(fresh_prefab_entity)
                    .collect(),
            })
            .collect();
        let bindings = snapshot
            .bind_entities(
                (0..job_sources.len()).map(fresh_job_entity),
                &synthetic_prefabs,
            )
            .expect("representative synthetic target bindings");
        assert!(
            bindings.issues().is_empty(),
            "{name}: {:?}",
            bindings.issues()
        );
        assert_eq!(
            bindings.len(),
            number(capture, "bindings_including_null"),
            "{name}"
        );
        assert_eq!(bindings.target(Entity::NULL), Some(Entity::NULL));
        assert_eq!(bindings.source(Entity::NULL), Some(Entity::NULL));
        check_reference_patch(
            &snapshot,
            &bindings,
            "ProjectM.Buff",
            Entity::new(152839, 4),
            8,
            Entity::new(152830, 4),
        );
        check_reference_patch(
            &snapshot,
            &bindings,
            "Unity.Entities.LinkedEntityGroup",
            Entity::new(86390, 3),
            0,
            Entity::new(86390, 3),
        );
        if name == "AutoSave_4.save.gz" {
            check_reference_patch(
                &snapshot,
                &bindings,
                "ProjectM.EntityOwner",
                Entity::new(559509, 3),
                0,
                Entity::new(36976, 1),
            );
        }
        let mut unknown = [0xff; 8];
        assert_eq!(bindings.patch_references(&mut unknown, 8, &[0]).unwrap(), 1);
        assert_eq!(unknown, [0; 8]);
        for (ordinal, source) in job_sources.into_iter().enumerate() {
            let target = fresh_job_entity(ordinal);
            assert_eq!(bindings.target(source), Some(target), "{name}/{ordinal}");
            assert_eq!(bindings.source(target), Some(source), "{name}/{ordinal}");
        }
        for source in prefab_sources {
            let target = fresh_prefab_entity(source);
            assert_eq!(bindings.target(source), Some(target), "{name}/{source:?}");
            assert_eq!(bindings.source(target), Some(source), "{name}/{target:?}");
        }
        println!(
            "{name}: {} original source handles mapped both ways with supplied synthetic targets",
            bindings.len()
        );
    }
}
