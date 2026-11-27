use rrising_content::spawn_chain::ChainDefinition;
#[test]
fn nested_chain_records_are_bounded() {
    let mut blob = vec![0; 156];
    blob[..4].copy_from_slice(&16i32.to_le_bytes());
    blob[4..8].copy_from_slice(&1i32.to_le_bytes());
    blob[16..20].copy_from_slice(&96i32.to_le_bytes());
    blob[20..24].copy_from_slice(&1i32.to_le_bytes());
    blob[60..64].copy_from_slice(&77i32.to_le_bytes());
    blob[144..148].copy_from_slice(&(-1i32).to_le_bytes());
    blob[152] = 1;
    let chain = ChainDefinition::read(&blob).unwrap();
    let main = chain.main_element().unwrap();
    assert_eq!(main.prefab_guid, 77);
    assert!(main.transitions[0].end_of_chain);
    assert_eq!(main.transitions[0].target, -1);
    assert!(ChainDefinition::read(&blob[..155]).is_err());
    blob[20..24].copy_from_slice(&(-1i32).to_le_bytes());
    assert!(ChainDefinition::read(&blob).is_err());
}
