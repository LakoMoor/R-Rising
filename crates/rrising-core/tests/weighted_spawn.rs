use rrising_core::weighted_spawn::{select, WeightedChain, WeightedGroup};
#[test]
fn selection_and_rng_match_isolated_original() {
    let data: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evidence/oracles/weighted_spawn_chain.json"
    ))
    .unwrap();
    let cases = data["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 1024);
    for (index, case) in cases.iter().enumerate() {
        let groups: Vec<_> = case["groups"]
            .as_array()
            .unwrap()
            .iter()
            .map(|g| WeightedGroup {
                weight: g["weight"].as_f64().unwrap() as f32,
                total_chain_weight: g["total_chain_weight"].as_f64().unwrap() as f32,
                chains: g["chains"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| WeightedChain {
                        guid: c["guid"].as_i64().unwrap() as i32,
                        weight: c["weight"].as_f64().unwrap() as f32,
                    })
                    .collect(),
            })
            .collect();
        let mut state = case["seed"].as_u64().unwrap() as u32;
        let guid = select(
            &groups,
            case["total_group_weight"].as_f64().unwrap() as f32,
            &mut state,
        )
        .unwrap();
        assert_eq!(
            i64::from(guid),
            case["guid"].as_i64().unwrap(),
            "guid case {index}"
        );
        assert_eq!(
            u64::from(state),
            case["state"].as_u64().unwrap(),
            "rng case {index}"
        );
    }
}
