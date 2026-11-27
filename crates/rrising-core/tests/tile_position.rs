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
