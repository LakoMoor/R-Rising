#![cfg(feature = "content")]
use rrising_content::spawn_chain::{
    AutoTransitionConfig, ChainDefinition, ChainElement, ChainTransition,
};
use rrising_core::transition_selection::{select, SelectionRequest};
use rrising_ecs::Entity;
fn fixed(text: &str) -> [u8; 32] {
    let mut bytes = [0; 32];
    bytes[..2].copy_from_slice(&(text.len() as u16).to_le_bytes());
    bytes[2..2 + text.len()].copy_from_slice(text.as_bytes());
    bytes
}
#[test]
fn original_selection_matches_candidate_bytes() {
    let data: serde_json::Value = serde_json::from_str(include_str!(
        "../../../evidence/oracles/transition_selection.json"
    ))
    .unwrap();
    let cases = data["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 400);
    let mut successful = 0;
    for (row, case) in cases.iter().enumerate() {
        let start = case["start"].as_i64().unwrap() as i32;
        let elements = case["elements"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, element)| ChainElement {
                index: i as i32,
                prefab_guid: element["guid"].as_i64().unwrap() as i32,
                use_auto_transition: element["auto"].as_bool().unwrap(),
                auto_transition: AutoTransitionConfig {
                    transition_id: fixed("auto"),
                    transition_index: 0,
                    attempt_cooldown: 0.5,
                    duration: element["duration"].as_f64().unwrap() as f32,
                },
                transitions: if i as i32 == start {
                    case["transitions"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|t| ChainTransition {
                            id: fixed(t["id"].as_str().unwrap()),
                            target: t["target"].as_i64().unwrap() as i32,
                            importance: t["importance"].as_i64().unwrap() as i32,
                            end_of_chain: t["end"].as_bool().unwrap(),
                        })
                        .collect()
                } else {
                    Vec::new()
                },
            })
            .collect();
        let chain = ChainDefinition {
            main_index: case["main"].as_i64().unwrap() as i32,
            elements,
        };
        let actual = select(
            &chain,
            &SelectionRequest {
                transition_id: fixed(case["requested"].as_str().unwrap()),
                start_element: start,
                loop_on_end: case["loop"].as_bool().unwrap(),
                forwarded_seconds: case["forwarded"].as_f64().unwrap(),
                local_to_world: std::array::from_fn(|i| i as f32),
                last_failed_importance: case["failed"].as_i64().map(|v| v as i32),
            },
            |guid| Some(Entity::new(guid + 1000, 3)),
        )
        .unwrap();
        assert_eq!(
            actual.importance,
            case["importance"].as_i64().unwrap() as i32,
            "row {row}"
        );
        assert_eq!(
            actual.candidate.is_some(),
            case["success"].as_bool().unwrap(),
            "row {row}"
        );
        let mut bytes = [0u8; 136];
        if let Some(candidate) = actual.candidate {
            successful += 1;
            bytes[..4].copy_from_slice(&candidate.element_index.to_le_bytes());
            for (i, value) in candidate.local_to_world.iter().enumerate() {
                bytes[4 + i * 4..8 + i * 4].copy_from_slice(&value.to_le_bytes());
            }
            bytes[68..72].copy_from_slice(&candidate.prefab.index.to_le_bytes());
            bytes[72..76].copy_from_slice(&candidate.prefab.version.to_le_bytes());
            bytes[76] = u8::from(candidate.destroy_chain_on_end);
            bytes[80..88].copy_from_slice(&candidate.forwarded_seconds.to_le_bytes());
            if let Some(config) = candidate.auto_transition {
                bytes[88] = 1;
                bytes[92..124].copy_from_slice(&config.transition_id);
                bytes[124..128].copy_from_slice(&config.transition_index.to_le_bytes());
                bytes[128..132].copy_from_slice(&config.attempt_cooldown.to_le_bytes());
                bytes[132..136].copy_from_slice(&config.duration.to_le_bytes());
            }
        }
        let expected: Vec<_> = case["result"]
            .as_str()
            .unwrap()
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect();
        assert_eq!(bytes.as_slice(), expected, "row {row}");
    }
    assert_eq!(successful, 232);
}
