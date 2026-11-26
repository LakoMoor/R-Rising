use rrising_core::player_movement::{get_movement, MovementInput};

#[test]
fn matches_original_replay_for_cameras_modes_and_thresholds() {
    let mut count = 0;
    for line in include_str!("../../../evidence/oracles/player_get_movement.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 17);
        let value = |i: usize| f32::from_bits(u32::from_str_radix(fields[i], 16).unwrap());
        let mut input = MovementInput {
            axis: [value(2), value(3)],
            mode: fields[0].parse().unwrap(),
            slow: fields[1] == "1",
            last_direction: [value(8), value(9), value(10)],
        };
        let actual = get_movement(&mut input, [value(4), value(5), value(6), value(7)]);
        for (i, value_out) in actual.into_iter().chain(input.last_direction).enumerate() {
            assert_eq!(
                value_out.to_bits(),
                value(11 + i).to_bits(),
                "case {count} field {i}: {line}"
            );
        }
        count += 1;
    }
    assert_eq!(count, 560);
}
