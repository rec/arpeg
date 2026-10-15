use arpeg_core::{
    Bank, Beat, Selection,
    chance::Chance,
    engine::{Arpeggiator, Retrigger},
    rhythm::Rhythm,
    timing_ports::TimingPorts,
    token::TokenPorts,
};
use serde_json::{Value, json};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn opaque_tokens_match_python_with_independent_overlapping_occurrences() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../conformance/tokens.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        for polling in [false, true] {
            let gate: Beat = case["gate"].as_str().unwrap().parse().unwrap();
            let mut engine = Arpeggiator::new(
                if case["latch"].as_bool().unwrap() {
                    Bank::LatchedReplace
                } else {
                    Bank::Held
                },
                Selection::Ascending,
                Rhythm::Grid {
                    swing: arpeg_core::Beat::new(1, 2),
                    step: Beat::new(1, 4),
                },
                TokenPorts {
                    timing: TimingPorts::new(gate, Beat::from_integer(1)),
                },
                Retrigger::OnEmpty,
                Chance::default(),
            )
            .unwrap();
            let mut events = Vec::new();
            let mut now = Beat::from_integer(0);
            for action in case["actions"].as_array().unwrap() {
                let at: Beat = action[1].as_str().unwrap().parse().unwrap();
                if polling {
                    while now + Beat::new(1, 17) < at {
                        now += Beat::new(1, 17);
                        events.extend(engine.advance(now).unwrap());
                    }
                }
                events.extend(match action[0].as_str().unwrap() {
                    "press" => engine
                        .press(
                            at,
                            action[2].as_u64().unwrap(),
                            i128::from(action[3].as_i64().unwrap()),
                            action[4].as_u64().unwrap(),
                        )
                        .unwrap(),
                    "release" => engine.release(at, action[2].as_u64().unwrap()).unwrap(),
                    "advance" => engine.advance(at).unwrap(),
                    "stop" => engine.stop(at).unwrap(),
                    _ => unreachable!(),
                });
                now = at;
            }
            let actual: Vec<_> = events
                .iter()
                .map(|e| {
                    json!([
                        e.at.to_string(),
                        if e.start { "on" } else { "off" },
                        e.id,
                        e.source_id,
                        case["tokens"][e.value as usize]
                    ])
                })
                .collect();
            assert_eq!(json!(actual), case["expected"], "{}", case["name"]);
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn descending_supports_the_full_signed_rank_range() {
    let mut engine = Arpeggiator::new(
        Bank::Held,
        Selection::Descending,
        Rhythm::Grid {
            swing: arpeg_core::Beat::new(1, 2),
            step: Beat::new(1, 4),
        },
        TokenPorts {
            timing: TimingPorts::new(Beat::new(4, 5), Beat::from_integer(1)),
        },
        Retrigger::OnEmpty,
        Chance::default(),
    )
    .unwrap();
    engine
        .press(Beat::from_integer(0), 1000, i128::MIN, 0)
        .unwrap();
    engine
        .press(Beat::from_integer(0), 2000, i128::MAX, 1)
        .unwrap();
    let events = engine.advance(Beat::new(1, 4)).unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.start)
            .map(|e| e.value)
            .collect::<Vec<_>>(),
        [1, 0]
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn generic_occurrences_retain_accents_without_reinterpreting_payloads() {
    use arpeg_core::rhythm::StepPatterns;
    let mut engine = Arpeggiator::new(
        Bank::Held,
        Selection::Ascending,
        Rhythm::Grid {
            step: Beat::new(1, 4),
            swing: Beat::new(1, 2),
        },
        TokenPorts {
            timing: TimingPorts::new(Beat::new(4, 5), Beat::from_integer(1)),
        },
        Retrigger::OnEmpty,
        Chance::default(),
    )
    .unwrap();
    engine.patterns = StepPatterns {
        accents: vec![
            Beat::new(1, 2),
            Beat::from_integer(0),
            Beat::from_integer(2),
        ],
        ..StepPatterns::default()
    };
    engine
        .press(Beat::from_integer(0), 1000, 5000, 777)
        .unwrap();
    let actual: Vec<_> = engine
        .advance(Beat::new(1, 2))
        .unwrap()
        .into_iter()
        .map(|e| (e.start, e.value, e.accent))
        .collect();
    assert_eq!(
        actual,
        [
            (true, 777, Beat::new(1, 2)),
            (false, 777, Beat::new(1, 2)),
            (true, 777, Beat::from_integer(2))
        ]
    );
}
