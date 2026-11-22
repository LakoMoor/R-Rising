//! Tests for `World::build` and `SpecialTypes` construction on synthetic data (not original data).
use rrising_ecs::{
    ArchetypeInput, ChunkInput, ComponentType, Entity, EntityIds, EntityQuery, SpecialTypes,
    TypeCategory, TypeIndex, TypeManager, TypeSpec, World, WorldInput,
};

struct T {
    tm: TypeManager,
    entity: TypeIndex,
    a: TypeIndex,
    tag: TypeIndex,
    disabled: TypeIndex,
    prefab: TypeIndex,
}

fn types() -> T {
    let mut tm = TypeManager::default();
    tm.register(TypeSpec::tag("<reserved null>")).unwrap();
    let mut es = TypeSpec::component_data("Entity", 8, 8);
    es.category = TypeCategory::EntityData;
    let entity = tm.register(es).unwrap();
    let a = tm.register(TypeSpec::component_data("A", 4, 4)).unwrap();
    let tag = tm.register(TypeSpec::tag("Tag")).unwrap();
    let disabled = tm.register(TypeSpec::tag("Disabled")).unwrap();
    let prefab = tm.register(TypeSpec::tag("Prefab")).unwrap();
    T {
        tm,
        entity,
        a,
        tag,
        disabled,
        prefab,
    }
}

fn special(t: &T) -> SpecialTypes {
    SpecialTypes {
        disabled: t.disabled,
        prefab: t.prefab,
        ..SpecialTypes::new(t.entity)
    }
}

#[test]
fn special_types_default_does_not_alias_type_zero() {
    let d = SpecialTypes::default();
    for v in [
        d.chunk_header,
        d.prefab,
        d.spawn_tag,
        d.destroy_tag,
        d.cleanup_entity,
        d.disabled,
        d.system_instance,
    ] {
        assert_eq!(v, SpecialTypes::UNSET);
        assert_ne!(v, TypeIndex::NULL);
    }
    assert!(!d.entity_type_is_set());
    // a store with an unset Entity type is refused
    assert!(rrising_ecs::EntityComponentStore::new(4, d).is_err());
    assert!(SpecialTypes::new(TypeIndex::new(1)).entity_type_is_set());
}

#[test]
fn builds_preserving_ids_and_component_bytes() {
    let t = types();
    let ids = vec![Entity::new(5, 3), Entity::new(2, 7), Entity::new(40, 1)];
    let data: Vec<u8> = [11u32, 22, 33]
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .collect();
    let input = WorldInput {
        archetypes: vec![ArchetypeInput { types: vec![t.a] }],
        chunks: vec![ChunkInput {
            shared_components: vec![],
            archetype: 0,
            entities: EntityIds::Preserved(ids.clone()),
            components: vec![(t.a, data)],
        }],
    };
    let (a, sp) = (t.a, special(&t));
    let w = World::build(t.tm, sp, 4, &input).unwrap();
    for (e, v) in ids.iter().zip([11u32, 22, 33]) {
        assert!(w.store.exists(*e).unwrap());
        assert_eq!(w.store.component_bytes(*e, a).unwrap(), &v.to_le_bytes());
    }
    // wrong version is dead; untouched slot keeps version 1
    assert!(!w.store.exists(Entity::new(5, 1)).unwrap());
    assert_eq!(w.store.get_entity_by_entity_index(3), Entity::new(3, 1));
    // the free list skips used slots: next fresh allocation is slot 0
    let mut s = w.store;
    let arch = w.archetype_map[0];
    assert_eq!(s.create_entity(arch).unwrap(), Entity::new(0, 1));
    assert_eq!(s.create_entity(arch).unwrap(), Entity::new(1, 1));
    assert_eq!(s.create_entity(arch).unwrap(), Entity::new(3, 1));
}

#[test]
fn rejects_duplicate_or_bad_ids_and_bad_byte_lengths() {
    let t = types();
    let sp = special(&t);
    let mk = |ids: Vec<Entity>, bytes: usize, tm: &TypeManager| {
        let input = WorldInput {
            archetypes: vec![ArchetypeInput { types: vec![t.a] }],
            chunks: vec![ChunkInput {
                shared_components: vec![],
                archetype: 0,
                entities: EntityIds::Preserved(ids),
                components: vec![(t.a, vec![0; bytes])],
            }],
        };
        World::build(tm.clone(), sp, 4, &input)
    };
    assert!(mk(vec![Entity::new(1, 1), Entity::new(1, 2)], 8, &t.tm).is_err());
    assert!(mk(vec![Entity::new(1, 0)], 4, &t.tm).is_err());
    assert!(mk(vec![Entity::new(-1, 1)], 4, &t.tm).is_err());
    assert!(mk(vec![Entity::new(1, 1)], 3, &t.tm).is_err());
    assert!(mk(vec![Entity::new(1, 1)], 4, &t.tm).is_ok());
}

#[test]
fn query_counts_on_synthetic_data_with_disabled_and_prefab() {
    let t = types();
    let (a, tag, dis, pre) = (t.a, t.tag, t.disabled, t.prefab);
    let chunk = |arch, n| ChunkInput {
        shared_components: vec![],
        archetype: arch,
        entities: EntityIds::Fresh(n),
        components: vec![],
    };
    let input = WorldInput {
        archetypes: vec![
            ArchetypeInput { types: vec![a] },
            ArchetypeInput {
                types: vec![a, tag],
            },
            ArchetypeInput {
                types: vec![a, dis],
            },
            ArchetypeInput {
                types: vec![a, pre],
            },
        ],
        // 3 + 2 + 4 + 5; archetype 0 split over two input chunks
        chunks: vec![
            chunk(0, 2),
            chunk(0, 1),
            chunk(1, 2),
            chunk(2, 4),
            chunk(3, 5),
        ],
    };
    let sp = special(&t);
    let w = World::build(t.tm, sp, 2, &input).unwrap();
    assert_eq!(w.entities.len(), 14);
    let q = EntityQuery::new(&[ComponentType::read_only(a)]);
    assert_eq!(
        w.query_count(&q),
        5,
        "Disabled and Prefab excluded by default"
    );
    let q_tag = EntityQuery::new(&[ComponentType::read_only(a), ComponentType::exclude(tag)]);
    assert_eq!(w.query_count(&q_tag), 3);
    let mut q_dis = q.clone();
    q_dis.include_disabled = true;
    assert_eq!(w.query_count(&q_dis), 9);
    let mut q_all = q_dis.clone();
    q_all.include_prefab = true;
    assert_eq!(w.query_count(&q_all), 14);
    // explicitly requiring Disabled still needs include_disabled in this port-side matcher
    let q_req = EntityQuery::new(&[ComponentType::read_only(dis)]);
    assert_eq!(w.query_count(&q_req), 0);
}

#[test]
fn world_restores_shared_groups_for_preserved_and_fresh_entities() {
    let mut t = types();
    let a = t.tm.register(TypeSpec::shared("SharedA", 4, 4)).unwrap();
    let b = t.tm.register(TypeSpec::shared("SharedB", 4, 4)).unwrap();
    let special = special(&t);
    let chunk = |entities, a_value, b_value| ChunkInput {
        archetype: 0,
        entities,
        components: vec![],
        // Deliberately reverse the type order to check type-based mapping.
        shared_components: vec![(b, b_value), (a, a_value)],
    };
    let input = WorldInput {
        archetypes: vec![ArchetypeInput { types: vec![a, b] }],
        chunks: vec![
            chunk(EntityIds::Preserved(vec![Entity::new(10, 2)]), 7, 8),
            chunk(EntityIds::Preserved(vec![Entity::new(11, 3)]), 7, 9),
            chunk(EntityIds::Fresh(1), 7, 8),
        ],
    };
    let w = World::build_with(t.tm, special, 16, &input, true).unwrap();
    let locate = |e: Entity| w.store.entity_in_chunk(e.index).unwrap().chunk;
    assert_eq!(locate(w.entities[0]), locate(w.entities[2]));
    assert_ne!(locate(w.entities[0]), locate(w.entities[1]));
    let stored = &w.store.archetypes()[w.archetype_map[0]];
    for (entity, expected_b) in [(w.entities[0], 8), (w.entities[1], 9)] {
        let list = w.store.chunk(locate(entity)).unwrap().list_index as usize;
        let layout = &stored.archetype.data.layout;
        for (slot, ty) in stored.archetype.types
            [layout.first_shared_component as usize..layout.first_chunk_component as usize]
            .iter()
            .enumerate()
        {
            let expected = if ty.type_index == a { 7 } else { expected_b };
            assert_eq!(
                stored
                    .chunk_data
                    .get_shared_component_value(slot, list)
                    .unwrap(),
                expected
            );
        }
    }
}

#[test]
fn diagnostic_hash_normalizes_blob_addresses_and_hashes_external_buffers() {
    fn make(address: u64) -> (World, Entity, TypeIndex) {
        let mut t = types();
        let blob =
            t.tm.register(TypeSpec::component_data("BlobRef", 8, 8))
                .unwrap();
        let buffer = t.tm.register(TypeSpec::buffer("BlobBuffer", 8, 1)).unwrap();
        let special = special(&t);
        let input = WorldInput {
            archetypes: vec![ArchetypeInput {
                types: vec![blob, buffer],
            }],
            chunks: vec![ChunkInput {
                archetype: 0,
                entities: EntityIds::Fresh(1),
                components: vec![(blob, address.to_le_bytes().to_vec())],
                shared_components: vec![],
            }],
        };
        let mut world = World::build(t.tm, special, 4, &input).unwrap();
        let entity = world.entities[0];
        let payload = [address.to_le_bytes(), address.to_le_bytes()].concat();
        world
            .store
            .set_buffer_payload(&world.types, entity, buffer, &payload)
            .unwrap();
        world
            .register_blob_fingerprint(blob, vec![0], &[(address, 123)])
            .unwrap();
        world
            .register_blob_fingerprint(buffer, vec![0], &[(address, 123)])
            .unwrap();
        (world, entity, buffer)
    }
    let (a, _, _) = make(0x1000);
    let (mut b, entity, buffer) = make(0x2000);
    assert_eq!(a.state_hash(), b.state_hash());
    let changed = [0x2000u64.to_le_bytes(), 0u64.to_le_bytes()].concat();
    b.store
        .set_buffer_payload(&b.types, entity, buffer, &changed)
        .unwrap();
    assert_ne!(a.state_hash(), b.state_hash());
}
