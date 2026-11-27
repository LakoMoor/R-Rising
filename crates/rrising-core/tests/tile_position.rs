use rrising_core::tile_position::{height, height_level};
#[test]
fn all_compressed_heights_match_original_client_leaves() {
    let mut count = 0;
    for line in include_str!("../../../evidence/oracles/tile_position_height.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        let compressed = fields[0].parse::<u16>().unwrap();
        assert_eq!(
            height(compressed).to_bits(),
            u32::from_str_radix(fields[1], 16).unwrap(),
            "height {compressed}"
        );
        assert_eq!(
            height_level(compressed),
            fields[2].parse::<u8>().unwrap(),
            "level {compressed}"
        );
        count += 1;
    }
    assert_eq!(count, 65536);
}

#[test]
fn baked_to_world_matches_original_function_replay() {
    use rrising_core::tile_position::TilePosition;
    let mut count = 0;
    for line in include_str!("../../../evidence/oracles/tile_position_world.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
    {
        let f: Vec<i64> = line
            .split_whitespace()
            .map(|s| s.parse().unwrap())
            .collect();
        let source = TilePosition {
            tile: [f[0] as i32, f[1] as i32],
            rotation: f[2] as u8,
            compressed_height: f[3] as u16,
        };
        let actual = source.to_world([f[4] as i32, f[5] as i32], f[6] as u8);
        assert_eq!(
            actual,
            TilePosition {
                tile: [f[7] as i32, f[8] as i32],
                rotation: f[9] as u8,
                compressed_height: f[10] as u16
            },
            "row {count}"
        );
        count += 1;
    }
    assert_eq!(count, 4096);
}
