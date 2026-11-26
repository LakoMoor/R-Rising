use rrising_core::move_facing::movement_facing;

#[test]
fn ordinary_direction_matches_original_instruction_replay() {
    let mut count = 0;
    for line in include_str!("../../../evidence/oracles/move_facing.tsv")
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let fields: Vec<_> = line
            .split('\t')
            .map(|v| u32::from_str_radix(v, 16).unwrap())
            .collect();
        assert_eq!(fields.len(), 5);
        let input = [f32::from_bits(fields[0]), f32::from_bits(fields[1])];
        let actual = movement_facing(input, false).unwrap();
        assert_eq!(
            actual.map(f32::to_bits),
            [fields[2], fields[3], fields[4]],
            "row {count}"
        );
        assert!(movement_facing(input, true).is_none());
        count += 1;
    }
    assert_eq!(count, 80);
    assert!(movement_facing([0., 0.], false).is_none());
    assert!(movement_facing([f32::NAN, 1.], false).is_none());
}
