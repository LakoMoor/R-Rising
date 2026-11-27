use rrising_core::chain_spawn::SpawnPose;
fn decode(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}
fn floats<const N: usize>(text: &str) -> [f32; N] {
    let bytes = decode(text);
    assert_eq!(bytes.len(), N * 4);
    std::array::from_fn(|i| f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
}
#[test]
fn spawn_transform_bytes_match_original_wrapper_and_matrix_helpers() {
    let data: serde_json::Value =
        serde_json::from_str(include_str!("../../../evidence/oracles/chain_spawn.json")).unwrap();
    let cases = data["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 256);
    for (row, case) in cases.iter().enumerate() {
        let pose = SpawnPose::new(
            floats(case["position"].as_str().unwrap()),
            floats(case["rotation"].as_str().unwrap()),
        );
        let ops = case["trace"].as_array().unwrap();
        assert_eq!(
            ops.iter()
                .map(|op| op["op"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "instantiate",
                "translation",
                "rotation",
                "local_transform",
                "local_to_world",
                "attach"
            ]
        );
        for (name, bytes) in [
            ("translation", pose.translation.as_slice()),
            ("rotation", pose.rotation.as_slice()),
            ("local_transform", pose.local_transform.as_slice()),
            ("local_to_world", pose.local_to_world.as_slice()),
        ] {
            let op = ops.iter().find(|op| op["op"] == name).unwrap();
            assert_eq!(
                bytes,
                decode(op["bytes"].as_str().unwrap()),
                "row {row} {name}"
            );
        }
        let attach = &ops[5];
        assert_eq!(attach["chain"], 0x200000007u64);
        assert_eq!(attach["child"], 0x300000058u64);
        assert_eq!(attach["prefab"], 0x100000063u64);
        assert_eq!(attach["index"], case["index"]);
        assert_eq!(
            decode(attach["forwarded"].as_str().unwrap()),
            case["forwarded"].as_f64().unwrap().to_le_bytes()
        );
        let config = decode(attach["auto"].as_str().unwrap());
        let mut expected_config = [0u8; 48];
        if case["auto"].as_bool().unwrap() {
            expected_config[0] = 1;
            expected_config[44..].copy_from_slice(&2f32.to_le_bytes());
        }
        assert_eq!(config, expected_config);
        assert_eq!(
            decode(attach["server_time"].as_str().unwrap()),
            5f64.to_le_bytes()
        );
    }
}
