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

#[test]
fn automatic_progress_matches_original_replay() {
    use rrising_core::auto_chain::{progress, AutoProgressElement};
    let data: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evidence/oracles/auto_chain_progress.json"
    ))
    .unwrap();
    let cases = data["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 1040);
    for (index, case) in cases.iter().enumerate() {
        let elements: Vec<_> = case["elements"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, e)| AutoProgressElement {
                enabled: e["enabled"].as_bool().unwrap(),
                duration: e["duration"].as_f64().unwrap() as f32,
                target: e["target"].as_i64().unwrap() as i32,
                end: e["end"].as_bool().unwrap(),
                event: [(i + 1) as u8; 32],
            })
            .collect();
        let result = progress(
            &elements,
            case["main"].as_i64().unwrap() as i32,
            case["current"].as_i64().unwrap() as i32,
            case["loop"].as_bool().unwrap(),
            case["stop"].as_bool().unwrap(),
            case["skip"].as_bool().unwrap(),
            case["surplus"].as_f64().unwrap(),
            128,
        )
        .unwrap();
        assert_eq!(
            result.success,
            case["success"].as_bool().unwrap(),
            "success {index}"
        );
        assert_eq!(
            i64::from(result.origin),
            case["origin"].as_i64().unwrap(),
            "origin {index}"
        );
        assert_eq!(
            i64::from(result.target),
            case["target"].as_i64().unwrap(),
            "target {index}"
        );
        let event: String = result.event.iter().map(|v| format!("{v:02x}")).collect();
        assert_eq!(event, case["event"].as_str().unwrap(), "event {index}");
        assert_eq!(
            result.forwarded.to_bits(),
            u64::from_str_radix(case["duration_bits"].as_str().unwrap(), 16).unwrap(),
            "forwarded {index}"
        );
    }
}

#[test]
fn zero_duration_cycle_reports_host_budget_exhaustion() {
    use rrising_core::auto_chain::{progress, AutoProgressElement};
    let elements = [AutoProgressElement {
        enabled: true,
        duration: 0.,
        target: 0,
        end: false,
        event: [0; 32],
    }];
    assert!(progress(&elements, 1, 0, true, false, false, 0., 16).is_err());
}
