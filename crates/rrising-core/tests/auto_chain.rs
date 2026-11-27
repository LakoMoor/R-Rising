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

#[test]
fn auto_path_matches_original_replay() {
    use rrising_core::auto_chain::{duration_until_main, AutoPathStep};
    let data: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evidence/oracles/auto_chain_path.json"
    ))
    .unwrap();
    let cases = data["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 404);
    for (index, case) in cases.iter().enumerate() {
        let steps: Vec<_> = case["elements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| AutoPathStep {
                enabled: e["enabled"].as_bool().unwrap(),
                duration: e["duration"].as_f64().unwrap() as f32,
                target: e["target"].as_i64().unwrap() as i32,
            })
            .collect();
        let result = duration_until_main(
            &steps,
            case["main"].as_i64().unwrap() as i32,
            case["start"].as_i64().unwrap() as i32,
        );
        assert_eq!(
            result.is_some(),
            case["success"].as_bool().unwrap(),
            "success {index}"
        );
        assert_eq!(
            result.unwrap_or(0.).to_bits(),
            u64::from_str_radix(case["duration_bits"].as_str().unwrap(), 16).unwrap(),
            "duration {index}"
        );
    }
}
