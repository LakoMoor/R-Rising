//! Differential tests against pure Unity.Entities leaves replayed from the original GameAssembly.dll
//! (x86-64 under Rosetta). Golden rows come from `tools/capture_entities.py`; only inputs and
//! outputs are stored (evidence/oracles/*.tsv).
use rrising_ecs::archetype::{ArchetypeData, ArchetypeFlags, ArchetypeLayout};
use rrising_ecs::chunk::ChunkIndex;
use rrising_ecs::type_table::{ComponentTypeInArchetype, TypeIndex};
use rrising_ecs::{Archetype, Entity, Hash128};

fn rows(src: &'static str) -> Vec<Vec<&'static str>> {
    src.lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| l.split('\t').collect())
        .collect()
}
fn hx(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap()
}
fn dec(s: &str) -> i32 {
    s.parse().unwrap()
}
fn flag(s: &str) -> bool {
    match s {
        "0" => false,
        "1" => true,
        other => panic!("bool {other}"),
    }
}
fn hash(s: &str) -> Hash128 {
    Hash128::new(hx(&s[0..8]), hx(&s[8..16]), hx(&s[16..24]), hx(&s[24..32]))
}

#[test]
fn type_index_getters_match_original() {
    let data = rows(include_str!(
        "../../../evidence/oracles/typeindex_flags.tsv"
    ));
    assert_eq!(data.len(), 2048);
    for r in data {
        assert_eq!(r.len(), 22);
        let raw = hx(r[0]);
        let t = TypeIndex::new(raw as i32);
        let want = [
            t.is_cleanup_component(),
            t.is_cleanup_shared_component(),
            t.is_cleanup_buffer_component(),
            t.is_component_type(),
            t.is_shared_component_type(),
            t.is_iequatable(),
            t.is_managed_component(),
            t.is_managed_shared_component(),
            t.is_managed_type(),
            t.is_zero_sized(),
            t.is_chunk_component(),
            t.is_enableable(),
            t.is_refcounted(),
            t.is_temporary_baking_type(),
            t.is_buffer(),
            t.has_native_container(),
            t.is_baking_only_type(),
            t.has_entity_references(),
            t.is_chunk_serializable(),
        ];
        for (i, w) in want.iter().enumerate() {
            assert_eq!(*w, flag(r[1 + i]), "getter column {i} for {raw:08x}");
        }
        assert_eq!(t.flags(), dec(r[20]) as u32, "flags {raw:08x}");
        assert_eq!(t.index(), dec(r[21]), "index {raw:08x}");
    }
}

#[test]
fn hash128_matches_original() {
    let data = rows(include_str!("../../../evidence/oracles/hash128.tsv"));
    assert_eq!(data.len(), 2048);
    for r in data {
        assert_eq!(r.len(), 8);
        let (a, b) = (hash(r[0]), hash(r[1]));
        assert_eq!(a.less_than(&b), flag(r[2]), "lt {r:?}");
        assert_eq!(a.greater_than(&b), flag(r[3]), "gt {r:?}");
        assert_eq!(a.compare_to(&b) as i32, dec(r[4]), "compare {r:?}");
        assert_eq!(a.get_hash_code(), dec(r[5]), "hash a {r:?}");
        assert_eq!(b.get_hash_code(), dec(r[6]), "hash b {r:?}");
        assert_eq!(a != b, flag(r[7]), "inequality {r:?}");
    }
}

#[test]
fn archetype_getters_match_original() {
    let data = rows(include_str!(
        "../../../evidence/oracles/archetype_getters.tsv"
    ));
    assert_eq!(data.len(), 2048);
    for r in data {
        assert_eq!(r.len(), 7 + 13 + 11);
        let flags = hx(r[0]) as u16;
        let layout = ArchetypeLayout {
            first_buffer_component: dec(r[1]),
            first_managed_component: dec(r[2]),
            first_tag_component: dec(r[3]),
            first_shared_component: dec(r[4]),
            first_chunk_component: dec(r[5]),
            chunk_capacity: 0,
            instance_size: 0,
            instance_size_with_overhead: 0,
            offsets: vec![],
            sizeofs: vec![],
            buffer_capacities: vec![],
            memory_order_to_archetype: vec![],
            archetype_to_memory_order: vec![],
        };
        let data = ArchetypeData {
            stable_hash: 0,
            bloom_filter_mask: 0,
            flags: ArchetypeFlags(flags),
            scalar_entity_patches: vec![],
            buffer_entity_patches: vec![],
            layout,
        };
        let mut a = Archetype::new(0, vec![], data);
        a.types_count = dec(r[6]);
        let l = a.layout();
        let bools = [
            a.cleanup_complete(),
            a.cleanup_needed(),
            a.disabled(),
            a.prefab(),
            a.spawn_tag(),
            a.destroy_tag(),
            a.has_chunk_header(),
            a.has_blob_asset_refs(),
            a.has_managed_entity_refs(),
            a.has_companion_components(),
            a.has_weak_asset_refs(),
            a.has_system_instance_components(),
            a.has_chunk_components(),
        ];
        for (i, w) in bools.iter().enumerate() {
            assert_eq!(*w, flag(r[7 + i]), "bool getter {i} flags {flags:04x}");
        }
        let ints = [
            l.num_native_component_data(),
            l.num_buffer_components(),
            l.num_managed_components(),
            l.num_tag_components(),
            l.num_shared_components(),
            l.num_chunk_components(a.types_count),
            l.non_zero_sized_types_count(),
            l.native_components_end(),
            l.buffer_components_end(),
            l.tag_components_end(),
            l.shared_components_end(),
        ];
        for (i, w) in ints.iter().enumerate() {
            assert_eq!(*w, dec(r[20 + i]), "int getter {i} row {r:?}");
        }
    }
}

#[test]
fn entity_chunk_and_component_compare_match_original() {
    let data = rows(include_str!("../../../evidence/oracles/entity_compare.tsv"));
    assert_eq!(data.len(), 2048);
    for r in data {
        assert_eq!(r.len(), 6);
        let (a, b) = (hx(r[0]) as i32, hx(r[1]) as i32);
        assert_eq!(
            Entity::new(a, 7).compare_to(Entity::new(b, 9)),
            dec(r[2]),
            "entity {r:?}"
        );
        assert_eq!(
            ChunkIndex::lt(ChunkIndex::new(a), ChunkIndex::new(b)),
            flag(r[3])
        );
        assert_eq!(ChunkIndex::new(a) > ChunkIndex::new(b), flag(r[4]));
        let (ta, tb) = (
            ComponentTypeInArchetype::new(TypeIndex::new(a)),
            ComponentTypeInArchetype::new(TypeIndex::new(b)),
        );
        assert_eq!(ta >= tb, flag(r[5]), "type_in_archetype >= {r:?}");
    }
}
