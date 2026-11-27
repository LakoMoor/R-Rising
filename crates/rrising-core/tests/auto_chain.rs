use rrising_core::auto_chain::AutoChainClock;
#[test]
fn clocks_match_complete_original_leaves() {
    let mut count = 0;
    for line in include_str!("../../../evidence/oracles/auto_chain_clock.tsv")
        .lines()
        .filter(|s| !s.starts_with('#'))
    {
        let fields: Vec<_> = line.split_whitespace().collect();
        let bits: Vec<u64> = fields[1..]
            .iter()
            .map(|s| u64::from_str_radix(s, 16).unwrap())
            .collect();
        let duration = f32::from_bits(bits[0] as u32);
        let now = f64::from_bits(bits[1]);
        let surplus = f64::from_bits(bits[2]);
        let clock = AutoChainClock::create(fields[0] == "1", duration, now, surplus);
        assert_eq!(clock.next_attempt.to_bits(), bits[3], "next row {count}");
        assert_eq!(
            clock.remaining(now).to_bits(),
            bits[4],
            "remaining row {count}"
        );
        assert_eq!(
            clock.passed(duration, now).to_bits(),
            bits[5],
            "passed row {count}"
        );
        count += 1;
    }
    assert_eq!(count, 240);
}
