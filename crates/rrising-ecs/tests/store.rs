//! Behaviour tests for `EntityComponentStore`. Derived from decompiled bodies (see `store.rs`);
//! these are unit tests of the port, NOT differential tests against the running original.
use rrising_ecs::{
    ComponentType, Entity, EntityComponentStore, EntityQuery, SpecialTypes, TypeCategory,
    TypeIndex, TypeManager, TypeSpec,
};

struct W {
    tm: TypeManager,
    entity: TypeIndex,
    a: TypeIndex,
    b: TypeIndex,
    tag: TypeIndex,
}

fn world() -> W {
    let mut tm = TypeManager::default();
    let mut es = TypeSpec::component_data("Entity", 8, 8);
    es.category = TypeCategory::EntityData;
    let entity = tm.register(es).unwrap();
    let a = tm.register(TypeSpec::component_data("A", 4, 4)).unwrap();
    let b = tm.register(TypeSpec::component_data("B", 8, 8)).unwrap();
    let tag = tm.register(TypeSpec::tag("Tag")).unwrap();
    W {
        tm,
        entity,
        a,
        b,
        tag,
    }
}

fn store(w: &W, cap: i32) -> EntityComponentStore {
    // The all-zero Default of SpecialTypes would alias every special slot to TypeIndex 0 (here the
    // Entity type) and set Disabled/Prefab/... flags on every archetype, so use an unused value.
    let none = TypeIndex::new(-1);
    EntityComponentStore::new(
        cap,
        SpecialTypes {
            chunk_header: none,
            prefab: none,
            spawn_tag: none,
            destroy_tag: none,
            cleanup_entity: none,
            disabled: none,
            entity_type: w.entity,
            system_instance: none,
        },
    )
    .unwrap()
}

#[test]
fn allocation_hands_out_sequential_indices_with_version_one() {
    let w = world();
    let mut s = store(&w, 4);
    let arch = s.get_or_create_archetype(&w.tm, &[w.a]).unwrap();
    let es = s.create_entities(arch, 3).unwrap();
    assert_eq!(
        es,
        vec![Entity::new(0, 1), Entity::new(1, 1), Entity::new(2, 1)]
    );
    assert!(es.iter().all(|e| s.exists(*e).unwrap()));
    assert_eq!(s.next_free_entity_index(), 3);
}

#[test]
fn destroy_bumps_version_and_old_handle_is_dead() {
    let w = world();
    let mut s = store(&w, 4);
    let arch = s.get_or_create_archetype(&w.tm, &[w.a]).unwrap();
    let e = s.create_entity(arch).unwrap();
    s.destroy_entity(e).unwrap();
    assert!(!s.exists(e).unwrap());
    assert_eq!(s.get_entity_by_entity_index(0), Entity::new(0, 2));
    // freed slot is the new free-list head: next allocation reuses it at version 2
    let e2 = s.create_entity(arch).unwrap();
    assert_eq!(e2, Entity::new(0, 2));
    assert!(!s.exists(e).unwrap());
    // destroying a stale handle is silently skipped
    s.destroy_entity(e).unwrap();
    assert!(s.exists(e2).unwrap());
}

#[test]
fn out_of_range_index_is_an_error() {
    let w = world();
    let s = store(&w, 4);
    assert!(s.exists(Entity::new(-1, 1)).is_err());
    assert!(s.exists(Entity::new(4, 1)).is_err());
    assert_eq!(s.get_entity_by_entity_index(4), Entity::NULL);
}

#[test]
fn allocating_the_last_slot_doubles_capacity() {
    // The head's link is -1 only for the last slot, so growth happens as that slot is handed out.
    let w = world();
    let mut s = store(&w, 1);
    let arch = s.get_or_create_archetype(&w.tm, &[w.a]).unwrap();
    let e0 = s.create_entity(arch).unwrap();
    assert_eq!(
        (e0, s.capacity(), s.next_free_entity_index()),
        (Entity::new(0, 1), 2, 1)
    );
    s.destroy_entity(e0).unwrap();
    assert_eq!(s.create_entity(arch).unwrap(), Entity::new(0, 2));
    assert_eq!(s.create_entity(arch).unwrap(), Entity::new(1, 1));
    assert_eq!((s.capacity(), s.next_free_entity_index()), (4, 2));
}

#[test]
fn capacity_limit_is_enforced() {
    let w = world();
    let sp = SpecialTypes {
        entity_type: w.entity,
        ..Default::default()
    };
    assert!(EntityComponentStore::new(0, sp).is_err());
    assert!(EntityComponentStore::new(0x800_0001, sp).is_err());
}

#[test]
fn swap_back_moves_tail_entity_and_component_bytes() {
    let w = world();
    let mut s = store(&w, 8);
    let arch = s.get_or_create_archetype(&w.tm, &[w.a, w.b]).unwrap();
    let es = s.create_entities(arch, 4).unwrap();
    for (i, e) in es.iter().enumerate() {
        s.set_component_bytes(*e, w.a, &(100 + i as u32).to_le_bytes())
            .unwrap();
    }
    s.destroy_entity(es[1]).unwrap();
    // tail entity (3) moved into slot 1
    assert_eq!(s.entity_in_chunk(3).unwrap().index_in_chunk, 1);
    assert_eq!(
        s.component_bytes(es[3], w.a).unwrap(),
        &103u32.to_le_bytes()
    );
    assert_eq!(
        s.component_bytes(es[2], w.a).unwrap(),
        &102u32.to_le_bytes()
    );
    let c = s.entity_in_chunk(0).unwrap().chunk;
    assert_eq!(s.chunk_entities(c).unwrap(), vec![es[0], es[3], es[2]]);
    assert_eq!(s.archetypes()[arch].archetype.entity_count, 3);
}

#[test]
fn batch_destroy_links_free_list_in_batch_order() {
    let w = world();
    let mut s = store(&w, 8);
    let arch = s.get_or_create_archetype(&w.tm, &[w.a]).unwrap();
    let es = s.create_entities(arch, 5).unwrap();
    s.destroy_entities(&[es[1], es[2]]).unwrap();
    assert_eq!(s.next_free_entity_index(), 1);
    assert_eq!(s.entity_in_chunk(1).unwrap().index_in_chunk, 2);
    assert_eq!(s.entity_in_chunk(2).unwrap().index_in_chunk, 5);
    assert_eq!(s.create_entity(arch).unwrap(), Entity::new(1, 2));
}

#[test]
fn chunks_fill_release_and_are_reused() {
    let w = world();
    let mut s = store(&w, 16);
    let arch = s.get_or_create_archetype(&w.tm, &[w.b]).unwrap();
    let cap = s.archetypes()[arch].archetype.chunk_capacity() as usize;
    let es = s.create_entities(arch, cap + 3).unwrap();
    assert_eq!(s.archetypes()[arch].chunk_data.count(), 2);
    let q = EntityQuery::new(&[ComponentType::read_write(w.b)]);
    assert_eq!(s.query_chunks(&q).len(), 2);
    // destroy everything in the second chunk: it is released
    s.destroy_entities(&es[cap..]).unwrap();
    assert_eq!(s.archetypes()[arch].chunk_data.count(), 1);
    assert_eq!(s.query_entities(&q).unwrap().len(), cap);
    // first chunk is full; a new create allocates a fresh chunk (reusing the freed slot)
    let _ = s.create_entity(arch).unwrap();
    assert_eq!(s.archetypes()[arch].chunk_data.count(), 2);
    s.destroy_entities(&es[..cap]).unwrap();
    assert_eq!(s.query_entities(&q).unwrap().len(), 1);
}

#[test]
fn query_include_and_exclude() {
    let w = world();
    let mut s = store(&w, 16);
    let a_only = s.get_or_create_archetype(&w.tm, &[w.a]).unwrap();
    let ab = s.get_or_create_archetype(&w.tm, &[w.a, w.b]).unwrap();
    let a_tag = s.get_or_create_archetype(&w.tm, &[w.a, w.tag]).unwrap();
    let e1 = s.create_entity(a_only).unwrap();
    let e2 = s.create_entity(ab).unwrap();
    let e3 = s.create_entity(a_tag).unwrap();
    let all_a = EntityQuery::new(&[ComponentType::read_write(w.a)]);
    assert_eq!(s.query_entities(&all_a).unwrap(), vec![e1, e2, e3]);
    let a_not_b = EntityQuery::new(&[ComponentType::read_only(w.a), ComponentType::exclude(w.b)]);
    assert_eq!(s.query_entities(&a_not_b).unwrap(), vec![e1, e3]);
    let a_and_b = EntityQuery::new(&[
        ComponentType::read_write(w.a),
        ComponentType::read_write(w.b),
    ]);
    assert_eq!(s.query_entities(&a_and_b).unwrap(), vec![e2]);
    // same type set resolves to the same archetype regardless of order
    assert_eq!(s.get_or_create_archetype(&w.tm, &[w.b, w.a]).unwrap(), ab);
}

#[test]
fn order_versions_advance() {
    let w = world();
    let mut s = store(&w, 4);
    let arch = s.get_or_create_archetype(&w.tm, &[w.a]).unwrap();
    let before = s.component_type_order_version(w.a);
    let e = s.create_entity(arch).unwrap();
    assert_eq!(s.component_type_order_version(w.a), before + 1);
    let eo = s.entity_order_version();
    s.destroy_entity(e).unwrap();
    assert_eq!(s.entity_order_version(), eo + 1);
    assert_eq!(s.component_type_order_version(w.a), before + 2);
}

#[test]
fn buffer_ownership_survives_compaction_and_clears_on_reuse() -> anyhow::Result<()> {
    let mut world = world();
    let buffer = world.tm.register(TypeSpec::buffer("TestBuffer", 4, 1))?;
    let mut store = store(&world, 4);
    let arch = store.get_or_create_archetype(&world.tm, &[buffer])?;
    let first = store.create_entity(arch)?;
    let last = store.create_entity(arch)?;
    store.set_buffer_payload(&world.tm, first, buffer, &[9; 12])?;
    store.set_buffer_payload(&world.tm, last, buffer, &[6; 16])?;
    store.destroy_entity(first)?;
    assert_eq!(store.buffer_payload(&world.tm, last, buffer)?, &[6; 16]);
    let reused = store.create_entity(arch)?;
    assert_eq!(reused.index, first.index);
    assert_ne!(reused.version, first.version);
    assert!(store.buffer_payload(&world.tm, first, buffer).is_err());
    assert_eq!(
        store.buffer_payload(&world.tm, reused, buffer)?,
        &[] as &[u8]
    );
    store.set_buffer_payload(&world.tm, last, buffer, &[7; 4])?;
    assert_eq!(store.buffer_payload(&world.tm, last, buffer)?, &[7; 4]);
    assert!(store
        .set_buffer_payload(&world.tm, last, buffer, &[1; 3])
        .is_err());
    assert_eq!(store.buffer_payload(&world.tm, last, buffer)?, &[7; 4]);
    Ok(())
}

#[test]
fn enabled_rows_filter_queries_and_survive_swap_back_and_slot_reuse() -> anyhow::Result<()> {
    let mut world = world();
    let mut spec = TypeSpec::tag("Gate");
    spec.flags |= TypeIndex::K_ENABLEABLE_MASK;
    spec.maximum_chunk_capacity = 128;
    let gate = world.tm.register(spec)?;
    let mut store = store(&world, 140);
    let arch = store.get_or_create_archetype(&world.tm, &[gate])?;
    let entities = store.create_entities(arch, 128)?;
    let last = entities[127];
    let first = entities[0];
    assert!(store.component_enabled(last, gate)?);
    let chunk = store.entity_in_chunk(last.index).unwrap().chunk;
    store.increment_global_system_version();
    store.set_component_enabled(last, gate, false)?;
    assert_eq!(
        store.chunk_change_version(chunk, gate)?,
        store.global_system_version()
    );
    let positive = EntityQuery {
        all: vec![gate],
        ..Default::default()
    };
    let negative = EntityQuery {
        none: vec![gate],
        ..Default::default()
    };
    assert_eq!(store.query_entities(&positive)?.len(), 127);
    assert_eq!(store.query_entities(&negative)?, [last]);
    store.destroy_entity(first)?;
    assert!(!store.component_enabled(last, gate)?);
    let next = store.create_entity(arch)?;
    assert_eq!(next.index, first.index);
    assert!(store.component_enabled(next, gate)?);
    assert_eq!(store.query_entities(&negative)?, [last]);
    let slot = store.archetypes()[arch]
        .archetype
        .index_of_type(gate)
        .unwrap();
    assert_eq!(
        store.archetypes()[arch]
            .chunk_data
            .get_chunk_disabled_count_for_type(slot, 0)?,
        1
    );
    store.set_component_enabled(last, gate, true)?;
    assert_eq!(store.query_entities(&positive)?.len(), 128);
    assert!(store.query_entities(&negative)?.is_empty());
    assert!(store.set_component_enabled(first, gate, false).is_err());
    Ok(())
}

#[test]
fn retained_backing_storage_follows_store_lifetime() {
    use std::sync::Arc;
    let w = world();
    let mut original = store(&w, 2);
    let backing = Arc::new(vec![1u8, 2, 3]);
    let weak = Arc::downgrade(&backing);
    original.retain_lifetime(backing.clone());
    drop(backing);
    let moved = original;
    assert!(weak.upgrade().is_some());
    drop(moved);
    assert!(weak.upgrade().is_none());
}
