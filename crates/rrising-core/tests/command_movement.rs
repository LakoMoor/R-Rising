use rrising_core::command_movement::{select_movement_direction, write_command_input};

fn unpack(value: &str) -> [f32; 2] {
    let bits = u64::from_str_radix(value, 16).unwrap();
    [
        f32::from_bits(bits as u32),
        f32::from_bits((bits >> 32) as u32),
    ]
}

#[test]
fn writes_only_observed_command_input_fields() {
    let mut raw = [0x5a; 96];
    raw[..4].copy_from_slice(&0x8000_0000u32.to_le_bytes());
    raw[4..8].copy_from_slice(&0x7fc0_1234u32.to_le_bytes());
    let mut command = [0xa5; 152];
    write_command_input(&mut command, &raw, None, None).unwrap();
    assert_eq!(&command[..8], &raw[..8]);
    assert_eq!(&command[20..116], &raw);
    assert!(command[8..20]
        .iter()
        .chain(&command[116..])
        .all(|&b| b == 0xa5));
    let before = command;
    assert!(write_command_input(&mut command, &raw[..95], None, None).is_err());
    assert_eq!(command, before);
    write_command_input(&mut command, &raw, Some([1.0, -1.0]), Some((true, false))).unwrap();
    assert_eq!(&command[..8], &[0; 8]);
}

#[test]
fn command_direction_matches_original_instruction_slices() {
    let mut count = 0;
    for row in include_str!("../../../evidence/oracles/command_move_selection.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        let fields: Vec<_> = row.split('\t').collect();
        let vector = unpack(fields[1]);
        let (actual, expected) = match fields[0] {
            "raw" => (
                select_movement_direction(vector, None, None),
                unpack(fields[2]),
            ),
            "present" => {
                let spell = (fields[2] == "1").then_some((fields[3] == "1", fields[4] == "1"));
                (
                    select_movement_direction([5.0, 6.0], Some(vector), spell),
                    unpack(fields[5]),
                )
            }
            _ => panic!("unknown oracle row"),
        };
        assert_eq!(
            actual.map(f32::to_bits),
            expected.map(f32::to_bits),
            "{row}"
        );
        count += 1;
    }
    assert_eq!(count, 36);
}
