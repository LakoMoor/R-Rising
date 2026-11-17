use std::io::Write;

use flate2::{write::GzEncoder, Compression};
use rrising_content::persistence::PersistentSave;

// Synthetic payloads; names/order and the 172-byte table come from the actual
// original autosaves. Original payloads are private and tested separately below.
fn fixture() -> Vec<u8> {
    container(&[
        ("SerializationJob_0.save", b"job0"),
        ("SerializationJob_1.save", b"job1"),
        ("SerializationJob_2.save", b"job2"),
        ("SerializationJob_3.save", b""),
        ("Header.save", b"header"),
        ("WorldData.save", b"world"),
        ("Systems.save", b"systems"),
    ])
}

fn container(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = 2i32.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(entries.len() as i32).to_le_bytes());
    for (name, data) in entries {
        let mut len = name.len();
        while len >= 128 {
            bytes.push((len as u8 & 0x7f) | 0x80);
            len >>= 7;
        }
        bytes.push(len as u8);
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&(data.len() as i32).to_le_bytes());
    }
    for (_, data) in entries {
        bytes.extend_from_slice(data);
    }
    bytes
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes).expect("gzip fixture");
    encoder.finish().expect("gzip finish")
}

#[test]
fn original_table_shape_preserves_order_and_empty_job() {
    let bytes = fixture();
    let save = PersistentSave::from_decompressed(bytes.clone()).expect("container");
    assert_eq!(save.table_bytes(), 172);
    assert_eq!(save.uncompressed_bytes(), bytes.len());
    assert_eq!(save.members().len(), 7);
    assert_eq!(save.members()[3].name, "SerializationJob_3.save");
    assert_eq!(save.member_data(3), Some(&b""[..]));
    assert_eq!(save.member_data(4), Some(&b"header"[..]));
    assert_eq!(save.members()[3].offset, save.members()[4].offset);
    assert_eq!(save.member_data(7), None);
}

#[test]
fn unknown_entries_and_multibyte_string_lengths_keep_table_order() {
    let name = "é".repeat(70); // 140 UTF-8 bytes, two-byte 7-bit length
    let bytes = container(&[
        ("Systems.save", b"S"),
        (&name, b"extra"),
        ("Header.save", b"H"),
        ("SerializationJob_x", b""),
        ("WorldData.save", b"W"),
    ]);
    let save = PersistentSave::from_decompressed(bytes).expect("long UTF-8 name");
    assert_eq!(save.members()[1].name, name);
    assert_eq!(save.member_data(1), Some(&b"extra"[..]));
    assert_eq!(save.member_data(3), Some(&b""[..]));
}

#[test]
fn invalid_and_incomplete_containers_are_refused() {
    let fixture = fixture();
    for size in 0..fixture.len() {
        assert!(
            PersistentSave::from_decompressed(fixture[..size].to_vec()).is_err(),
            "accepted prefix {size}"
        );
    }
    for (offset, word) in [
        (0, 1i32),
        (0, 3),
        (4, -1),
        (4, i32::MAX),
        (32, -1),
        (32, i32::MAX),
    ] {
        let mut bytes = fixture.clone();
        // First name is 23 bytes: first size word is at offset 32.
        bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
        assert!(PersistentSave::from_decompressed(bytes).is_err());
    }
    let mut bytes = fixture.clone();
    bytes.push(0);
    assert!(PersistentSave::from_decompressed(bytes).is_err());
    let mut bytes = fixture.clone();
    bytes[8..13].copy_from_slice(&[0xff, 0xff, 0xff, 0xff, 0x08]);
    assert!(PersistentSave::from_decompressed(bytes).is_err());
    let mut bytes = fixture;
    bytes[9] = 0xff;
    assert!(PersistentSave::from_decompressed(bytes).is_err());
    for entries in [
        vec![
            ("Header.save", &b"H"[..]),
            ("WorldData.save", b"W"),
            ("Systems.save", b"S"),
        ],
        vec![
            ("SerializationJob_0", &b""[..]),
            ("WorldData.save", b"W"),
            ("Systems.save", b"S"),
        ],
        vec![("Header.save", &b"H"[..]), ("Header.save", b"duplicate")],
    ] {
        assert!(PersistentSave::from_decompressed(container(&entries)).is_err());
    }
}

#[test]
fn gzip_checks_checksum_completion_and_allocation_limit() {
    let bytes = fixture();
    let compressed = gzip(&bytes);
    let save = PersistentSave::read_gzip(&compressed[..], bytes.len()).expect("exact limit");
    assert_eq!(save.member_data(6), Some(&b"systems"[..]));
    assert!(PersistentSave::read_gzip(&compressed[..], bytes.len() - 1).is_err());
    assert!(PersistentSave::read_gzip(&compressed[..], usize::MAX).is_err());
    let mut bad = compressed.clone();
    let crc_offset = bad.len() - 8;
    bad[crc_offset] ^= 1;
    assert!(PersistentSave::read_gzip(&bad[..], bytes.len()).is_err());
    assert!(PersistentSave::read_gzip(&compressed[..compressed.len() - 1], bytes.len()).is_err());
    let mut trailing = compressed.clone();
    trailing.extend_from_slice(&compressed);
    assert!(PersistentSave::read_gzip(&trailing[..], bytes.len()).is_err());
}

#[test]
#[ignore = "requires the two captured original server saves and RRISING_SAVE_DIR"]
fn original_server_save_members_match_captured_observations() {
    let root =
        std::path::PathBuf::from(std::env::var_os("RRISING_SAVE_DIR").expect("RRISING_SAVE_DIR"));
    let observations: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evidence/oracles/save_containers.json"
    ))
    .expect("observation metadata");
    for row in observations.as_array().expect("rows") {
        let path = root.join(row["file"].as_str().expect("filename"));
        let save = PersistentSave::open(&path).expect("original save");
        assert_eq!(
            save.table_bytes() as u64,
            row["table_bytes"].as_u64().expect("table")
        );
        assert_eq!(
            save.uncompressed_bytes() as u64,
            row["uncompressed_bytes"].as_u64().expect("size")
        );
        let members = row["members"].as_array().expect("members");
        assert_eq!(save.members().len(), members.len());
        for (i, (member, expected)) in save.members().iter().zip(members).enumerate() {
            assert_eq!(member.name, expected["name"].as_str().expect("name"));
            assert_eq!(
                member.offset as u64,
                expected["offset"].as_u64().expect("offset")
            );
            assert_eq!(member.size as u64, expected["size"].as_u64().expect("size"));
            let hash = save
                .member_data(i)
                .expect("data")
                .iter()
                .fold(0xcbf29ce484222325u64, |h, b| {
                    (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
                });
            assert_eq!(
                format!("{hash:016x}"),
                expected["fnv1a64"].as_str().expect("hash")
            );
        }
        println!(
            "{}: {} bytes, {} table bytes, {} members, all payload fingerprints match",
            path.display(),
            save.uncompressed_bytes(),
            save.table_bytes(),
            save.members().len()
        );
    }
}
