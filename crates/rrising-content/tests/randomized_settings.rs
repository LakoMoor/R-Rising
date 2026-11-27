use rrising_content::randomized_settings::RandomizedSettings;
#[test]
fn nested_settings_arrays_are_bounded() {
    let mut data = vec![0u8; 40];
    data[..4].copy_from_slice(&16i32.to_le_bytes());
    data[4..8].copy_from_slice(&1i32.to_le_bytes());
    data[8..12].copy_from_slice(&2f32.to_le_bytes());
    data[16..20].copy_from_slice(&16i32.to_le_bytes());
    data[20..24].copy_from_slice(&1i32.to_le_bytes());
    data[24..28].copy_from_slice(&2f32.to_le_bytes());
    data[28..32].copy_from_slice(&3f32.to_le_bytes());
    data[32..36].copy_from_slice(&55i32.to_le_bytes());
    data[36..40].copy_from_slice(&3f32.to_le_bytes());
    let value = RandomizedSettings::read(&data).unwrap();
    assert_eq!(value.groups[0].chains[0].guid, 55);
    assert_eq!(value.groups[0].chains[0].weight, 3.);
    assert!(RandomizedSettings::read(&data[..39]).is_err());
    data[20..24].copy_from_slice(&(-1i32).to_le_bytes());
    assert!(RandomizedSettings::read(&data).is_err());
}
