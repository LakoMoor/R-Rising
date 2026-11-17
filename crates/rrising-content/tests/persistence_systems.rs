use rrising_content::persistence::{PersistentSave, SavedSystems};
use serde_json::Value;

fn prefix(elapsed_time: f64, slots: i32) -> Vec<u8> {
    let mut bytes = elapsed_time.to_le_bytes().to_vec();
    bytes.extend_from_slice(&slots.to_le_bytes());
    bytes
}

fn append_slot(bytes: &mut Vec<u8>, identifier: &[u8], payload: &[u8]) {
    bytes.push((u8::try_from(identifier.len()).expect("identifier length") << 1) | 1);
    bytes.extend_from_slice(identifier);
    bytes.extend_from_slice(
        &(i32::try_from(payload.len()).expect("payload length") * 8).to_le_bytes(),
    );
    bytes.extend_from_slice(payload);
}

#[test]
fn reads_packed_presence_flags_and_preserves_unknown_payloads() {
    let mut bytes = prefix(-0.0, 4);
    bytes.extend_from_slice(&[0b0001_1110, 0b1111_1110]);
    bytes.extend_from_slice("éframe".as_bytes());
    bytes.extend_from_slice(&16i32.to_le_bytes());
    bytes.extend_from_slice(&[12, 34]);
    append_slot(&mut bytes, b"WorldFrame", &(-123i32).to_le_bytes());
    bytes.push(0b1111_1110);

    let systems = SavedSystems::read(&bytes).expect("packed system member");
    assert_eq!(systems.elapsed_time.to_bits(), (-0.0f64).to_bits());
    assert_eq!(systems.slot_count, 4);
    assert_eq!(systems.records.len(), 2);
    assert_eq!(systems.records[0].slot_index, 1);
    assert_eq!(systems.records[0].offset_bits, 97);
    assert_eq!(systems.records[0].identifier, "éframe");
    assert_eq!(systems.records[0].payload_bits, 16);
    assert_eq!(systems.records[0].payload, [12, 34]);
    assert_eq!(systems.records[1].slot_index, 2);
    assert_eq!(systems.world_frame().expect("known frame"), Some(-123));
}

#[test]
fn rejects_truncation_and_invalid_lengths() {
    let mut bytes = prefix(4.0, 1);
    append_slot(&mut bytes, b"WorldFrame", &45i32.to_le_bytes());
    for end in 0..bytes.len() {
        assert!(SavedSystems::read(&bytes[..end]).is_err(), "prefix {end}");
    }
    let payload_bits_offset = 12 + 1 + b"WorldFrame".len();
    for length in [-1i32, 31, i32::MAX - 7] {
        let mut invalid = bytes.clone();
        invalid[payload_bits_offset..payload_bits_offset + 4]
            .copy_from_slice(&length.to_le_bytes());
        assert!(
            SavedSystems::read(&invalid).is_err(),
            "payload length {length}"
        );
    }
    let mut invalid = bytes.clone();
    invalid[12] = 0xff;
    assert!(SavedSystems::read(&invalid).is_err());
    let mut invalid = bytes.clone();
    invalid[13] = 0xff;
    assert!(SavedSystems::read(&invalid).is_err());
    bytes.push(0);
    assert!(SavedSystems::read(&bytes).is_err());
    assert!(SavedSystems::read(&prefix(0.0, -1)).is_err());
    assert!(SavedSystems::read(&prefix(0.0, i32::MAX)).is_err());
}

#[test]
fn typed_frame_requires_an_unambiguous_signed_word() {
    assert_eq!(
        SavedSystems::read(&prefix(0.0, 0))
            .expect("empty systems")
            .world_frame()
            .expect("missing frame"),
        None
    );
    let mut bad_frame = prefix(0.0, 1);
    append_slot(&mut bad_frame, b"WorldFrame", &[1, 2, 3]);
    assert!(SavedSystems::read(&bad_frame)
        .expect("opaque record")
        .world_frame()
        .is_err());
    let mut duplicate = prefix(0.0, 2);
    append_slot(&mut duplicate, b"WorldFrame", &[0; 4]);
    append_slot(&mut duplicate, b"WorldFrame", &[0; 4]);
    assert!(SavedSystems::read(&duplicate)
        .expect("opaque records")
        .world_frame()
        .is_err());
}

fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

#[test]
#[ignore = "requires two captured original server saves and RRISING_SAVE_DIR"]
fn original_system_members_roundtrip_with_saved_elapsed_time_and_frame() {
    let root =
        std::path::PathBuf::from(std::env::var_os("RRISING_SAVE_DIR").expect("RRISING_SAVE_DIR"));
    let golden: Value =
        serde_json::from_str(include_str!("../../../evidence/oracles/save_systems.json"))
            .expect("captured systems evidence");
    for capture in golden["saves"].as_array().expect("captured saves") {
        let name = capture["file"].as_str().expect("capture file");
        let save = PersistentSave::open(&root.join(name)).expect("original autosave");
        let bytes = save
            .named_member("Systems.save")
            .expect("original system bytes");
        let systems = SavedSystems::read(bytes).expect("original systems");
        let expected_records = golden["records"]
            .as_array()
            .expect("expected system records");
        assert_eq!(Some(bytes.len() as u64), golden["bytes"].as_u64(), "{name}");
        assert_eq!(
            Some(systems.slot_count as u64),
            golden["slot_count"].as_u64(),
            "{name}"
        );
        assert_eq!(systems.records.len(), expected_records.len(), "{name}");
        assert_eq!(
            format!("{:016x}", systems.elapsed_time.to_bits()),
            capture["elapsed_time_f64_bits"]
                .as_str()
                .expect("elapsed time bits"),
            "{name}"
        );
        assert_eq!(
            systems
                .world_frame()
                .expect("world frame record")
                .map(i64::from),
            capture["world_frame"].as_i64(),
            "{name}"
        );
        let mut roundtrip = prefix(systems.elapsed_time, systems.slot_count as i32);
        for (slot_index, (actual, expected)) in
            systems.records.iter().zip(expected_records).enumerate()
        {
            assert_eq!(actual.slot_index, slot_index);
            assert_eq!(Some(actual.identifier), expected[0].as_str());
            assert_eq!(
                actual.offset_bits,
                expected[1].as_u64().expect("record byte offset") * 8
            );
            assert_eq!(Some(actual.payload_offset as u64), expected[2].as_u64());
            assert_eq!(Some(actual.payload.len() as u64), expected[3].as_u64());
            assert_eq!(actual.payload_bits, actual.payload.len() * 8);
            let exact = &bytes[actual.payload_offset..actual.payload_offset + actual.payload.len()];
            assert!(std::ptr::eq(actual.payload, exact));
            let expected_hash = expected[4]
                .as_str()
                .or(capture["world_frame_fnv1a64"].as_str())
                .expect("expected payload hash");
            assert_eq!(
                fingerprint(actual.payload),
                expected_hash,
                "{name}/{}",
                actual.identifier
            );
            append_slot(&mut roundtrip, actual.identifier.as_bytes(), actual.payload);
        }
        assert_eq!(roundtrip, bytes, "{name}");
        println!(
            "{name}: all {} system records roundtrip across {} original bytes",
            systems.records.len(),
            bytes.len()
        );
    }
}
