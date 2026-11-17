use rrising_core::pathfinding::{try_get_target_position_from_waypoints, PathWaypoints};

#[test]
fn waypoint_selection_matches_original_instruction_replay() {
    let mut count = 0;
    for line in include_str!("../../../evidence/oracles/pathfinding.tsv").lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 14);
        let hex = |at| u32::from_str_radix(fields[at], 16).unwrap();
        let float = |at| f32::from_bits(hex(at));
        let mut bytes = [0xa5u8; 20];
        for i in 0..4 {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&hex(i).to_le_bytes());
        }
        bytes[16] = fields[4].parse().unwrap();
        bytes[17] = fields[5].parse().unwrap();
        let points = PathWaypoints::from_bytes(&bytes).unwrap();
        let mut target = [f32::from_bits(0x4f123456), f32::from_bits(0xcf123456)];
        let found = try_get_target_position_from_waypoints(
            &points,
            [float(6), float(7)],
            [float(8), float(9)],
            float(10),
            &mut target,
        );
        assert_eq!(found, fields[11] == "1", "row {count}");
        assert_eq!(target.map(f32::to_bits), [hex(12), hex(13)], "row {count}");
        count += 1;
    }
    assert_eq!(count, 4096);
    assert!(PathWaypoints::from_bytes(&[0; 19]).is_err());
}
