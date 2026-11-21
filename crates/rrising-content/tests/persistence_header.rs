use rrising_content::persistence::{PersistenceHeader, PersistentSave};

fn word(bytes: &mut Vec<u8>, value: i32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn fixture() -> Vec<u8> {
    let mut bytes = Vec::new();
    for v in [7, 4, 1, 1, 13, 99712] {
        word(&mut bytes, v);
    }
    bytes.extend_from_slice(&[1, 2, 0, 0xfe]); // bool false, ignored alignment bits
    word(&mut bytes, 1); // prefab count
    for v in [-123, 77, 3, 1, 78, 4] {
        word(&mut bytes, v);
    }
    word(&mut bytes, 2); // persistent count; UNKNOWN_0 is a valid record
    bytes.extend_from_slice(&0u64.to_le_bytes());
    word(&mut bytes, 0);
    word(&mut bytes, 0);
    bytes.extend_from_slice(&0x809388619ae080d7u64.to_le_bytes());
    word(&mut bytes, 40);
    word(&mut bytes, 0x20be6);
    word(&mut bytes, 2); // required count
    for (name, index) in [("UNKNOWN_0".to_owned(), 0), ("é".repeat(130), 0x20be6)] {
        // 9-bit byte length, then seven nonzero padding bits. This is not u16 length.
        bytes.extend_from_slice(&((name.len() as u16) | 0xfe00).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        word(&mut bytes, index);
    }
    word(&mut bytes, 1); // blob count
    word(&mut bytes, 77);
    word(&mut bytes, 3);
    bytes.extend_from_slice(&0x809388619ae080d7u64.to_le_bytes());
    word(&mut bytes, 8);
    word(&mut bytes, -1);
    bytes.extend_from_slice(&0xffff_ffff_1234_5678u64.to_le_bytes());
    bytes
}

#[test]
fn pinned_header_schema_preserves_handles_flags_and_alignment() {
    let header = PersistenceHeader::read(&fixture()).expect("header fixture");
    assert!(!header.using_server_only_subscenes);
    assert_eq!(header.game_version, [1, 1, 13, 99712]);
    assert_eq!(header.prefabs[0].prefab_guid, -123);
    assert_eq!(header.prefabs[0].entity.version, 3);
    assert_eq!(header.prefabs[0].linked_entities[0].index, 78);
    assert_eq!(header.persistent_components[0].stable_type_hash, 0);
    assert_eq!(
        header.persistent_components[1].saved_type_index.value,
        0x20be6
    );
    let name = "é".repeat(130);
    assert_eq!(header.required_components[1].full_name, name);
    assert_eq!(
        header
            .persistent_component_named(&name)
            .expect("join")
            .component_size,
        40
    );
    assert!(header.persistent_component_named("missing").is_none());
    assert_eq!(header.blobs[0].element_index, -1); // signed, not dereferenced
    assert_eq!(
        header.blobs[0].saved_blob_asset_address,
        0xffff_ffff_1234_5678
    );
}

#[test]
fn truncated_headers_invalid_versions_and_oversized_counts_are_refused() {
    let fixture = fixture();
    for size in 0..fixture.len() {
        assert!(
            PersistenceHeader::read(&fixture[..size]).is_err(),
            "prefix {size}"
        );
    }
    for (offset, value) in [
        (0, 8i32),
        (4, 3),
        (28, -1),
        (28, i32::MAX),
        (44, -1),
        (44, i32::MAX),
    ] {
        let mut bytes = fixture.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(PersistenceHeader::read(&bytes).is_err());
    }
    let mut trailing = fixture;
    trailing.push(0);
    assert!(PersistenceHeader::read(&trailing).is_err());
}

#[test]
fn fixed_string_capacity_and_utf8_are_checked_after_nine_bit_length() {
    let make = |length: u16, name: &[u8]| {
        let mut bytes = fixture()[..28].to_vec();
        for n in [0, 0, 1] {
            word(&mut bytes, n);
        }
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(name);
        word(&mut bytes, 0);
        word(&mut bytes, 0);
        bytes
    };
    assert!(PersistenceHeader::read(&make(509, &vec![b'x'; 509])).is_ok());
    assert!(PersistenceHeader::read(&make(510, &vec![b'x'; 510])).is_err());
    assert!(PersistenceHeader::read(&make(511, &vec![b'x'; 511])).is_err());
    assert!(PersistenceHeader::read(&make(1, &[0xff])).is_err());
}

// Test-only canonical re-encoding verifies every parsed field against actual
// original bytes, without introducing a production save writer.
fn canonical_bytes(h: &PersistenceHeader) -> Vec<u8> {
    let mut b = Vec::new();
    for v in [h.serialization_format_version, h.persistence_version] {
        word(&mut b, v);
    }
    for v in h.game_version {
        word(&mut b, v);
    }
    b.extend_from_slice(&[
        h.application_type,
        h.conversion_target,
        h.prefab_conversion_method,
        u8::from(h.using_server_only_subscenes),
    ]);
    word(&mut b, h.prefabs.len() as i32);
    for p in &h.prefabs {
        for v in [
            p.prefab_guid,
            p.entity.index,
            p.entity.version,
            p.linked_entities.len() as i32,
        ] {
            word(&mut b, v);
        }
        for e in &p.linked_entities {
            word(&mut b, e.index);
            word(&mut b, e.version);
        }
    }
    word(&mut b, h.persistent_components.len() as i32);
    for c in &h.persistent_components {
        b.extend_from_slice(&c.stable_type_hash.to_le_bytes());
        word(&mut b, c.component_size);
        word(&mut b, c.saved_type_index.value);
    }
    word(&mut b, h.required_components.len() as i32);
    for c in &h.required_components {
        b.extend_from_slice(&(c.full_name.len() as u16).to_le_bytes());
        b.extend_from_slice(c.full_name.as_bytes());
        word(&mut b, c.saved_type_index.value);
    }
    word(&mut b, h.blobs.len() as i32);
    for blob in &h.blobs {
        word(&mut b, blob.entity.index);
        word(&mut b, blob.entity.version);
        b.extend_from_slice(&blob.component_stable_type_hash.to_le_bytes());
        word(&mut b, blob.blob_field_offset);
        word(&mut b, blob.element_index);
        b.extend_from_slice(&blob.saved_blob_asset_address.to_le_bytes());
    }
    b
}

#[test]
#[ignore = "requires the two captured original server saves and RRISING_SAVE_DIR"]
fn original_save_headers_match_every_field_and_runtime_registry() {
    use std::collections::BTreeSet;
    let root =
        std::path::PathBuf::from(std::env::var_os("RRISING_SAVE_DIR").expect("RRISING_SAVE_DIR"));
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("../../../evidence/oracles/save_header.json"))
            .expect("captured header metadata");
    for name in golden["original_save_files"].as_array().expect("files") {
        let save = PersistentSave::open(&root.join(name.as_str().expect("name"))).expect("save");
        let header = save.header().expect("header");
        let raw = save
            .member_data(
                save.members()
                    .iter()
                    .position(|m| m.name == "Header.save")
                    .expect("header member"),
            )
            .expect("raw");
        assert_eq!(canonical_bytes(&header), raw);
        let hash = raw.iter().fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        });
        assert_eq!(
            format!("{hash:016x}"),
            golden["fnv1a64"].as_str().expect("hash")
        );
        for (key, actual) in [
            ("bytes", raw.len()),
            ("prefab_count", header.prefabs.len()),
            (
                "linked_entity_count",
                header.prefabs.iter().map(|p| p.linked_entities.len()).sum(),
            ),
            (
                "persistent_component_count",
                header.persistent_components.len(),
            ),
            ("required_component_count", header.required_components.len()),
            ("blob_count", header.blobs.len()),
            (
                "null_blob_addresses",
                header
                    .blobs
                    .iter()
                    .filter(|b| b.saved_blob_asset_address == 0)
                    .count(),
            ),
        ] {
            assert_eq!(actual as u64, golden[key].as_u64().expect(key));
        }
        let required: BTreeSet<_> = header
            .required_components
            .iter()
            .map(|c| c.saved_type_index)
            .collect();
        assert_eq!(required.len(), header.required_components.len());
        assert!(header
            .persistent_components
            .iter()
            .all(|c| required.contains(&c.saved_type_index)));
        for row in golden["movement_components"].as_array().expect("movement") {
            let c = header
                .persistent_component_named(row["name"].as_str().expect("name"))
                .expect("component");
            assert_eq!(
                format!("{:016x}", c.stable_type_hash),
                row["stable_type_hash"].as_str().expect("hash")
            );
            assert_eq!(
                i64::from(c.component_size),
                row["component_size"].as_i64().expect("size")
            );
            assert_eq!(
                i64::from(c.saved_type_index.value),
                row["saved_type_index"].as_i64().expect("index")
            );
        }
        println!("{}: entire {}-byte header canonical roundtrip matches original, all {} component indices join, six movement layouts confirmed", name, raw.len(), header.persistent_components.len());
    }
}

#[test]
#[ignore = "requires a captured original server save"]
fn original_blob_metadata_references_saved_prefab_members() {
    use std::collections::{BTreeMap, BTreeSet};
    let path = std::env::var_os("RRISING_SAVE_FILE").expect("set RRISING_SAVE_FILE");
    let save = PersistentSave::open(std::path::Path::new(&path)).expect("open save");
    let header = save.header().expect("read header");
    let prefabs: BTreeSet<_> = header
        .prefabs
        .iter()
        .flat_map(|prefab| {
            std::iter::once(prefab.entity).chain(prefab.linked_entities.iter().copied())
        })
        .map(|entity| (entity.index, entity.version))
        .collect();
    let mut types = BTreeMap::new();
    let mut matched = 0;
    for blob in &header.blobs {
        if prefabs.contains(&(blob.entity.index, blob.entity.version)) {
            matched += 1;
        }
        *types
            .entry(blob.component_stable_type_hash)
            .or_insert(0usize) += 1;
    }
    println!(
        "blob records={} prefab records={} types={}",
        header.blobs.len(),
        matched,
        types.len()
    );
    assert_eq!(matched, header.blobs.len());
}
