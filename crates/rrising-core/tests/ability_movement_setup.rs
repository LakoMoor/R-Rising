use rrising_core::movement::setup_ability_movement;

#[test]
fn setup_matches_original_instruction_slices() {
    let mut count = 0;
    for line in include_str!("../../../evidence/oracles/ability_movement_setup.tsv")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 3);
        let mut input = [0u8; 96];
        for (part, value) in input[..8]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(&fields[..2])
        {
            part.copy_from_slice(&u32::from_str_radix(value, 16).unwrap().to_le_bytes());
        }
        let expected: Vec<u8> = fields[2]
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|v| u8::from_str_radix(std::str::from_utf8(v).unwrap(), 16).unwrap())
            .collect();
        let mut movement = [0x5au8; 104];
        setup_ability_movement(&mut movement, &input).unwrap();
        assert_eq!(movement.as_slice(), expected, "case {count}");
        count += 1;
    }
    assert_eq!(count, 36);
    assert!(setup_ability_movement(&mut [0; 103], &[0; 96]).is_err());
    assert!(setup_ability_movement(&mut [0; 104], &[0; 95]).is_err());
}
