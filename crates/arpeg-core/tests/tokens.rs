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

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn conditional_passes_follow_transport_through_silence_and_chord_edits() {
    use arpeg_core::rhythm::{PassCondition, PatternStep, StepPatterns};
    for seek in [false, true] {
        let mut engine = Arpeggiator::new(
            Bank::Held,
            Selection::Ascending,
            Rhythm::Pattern {
                cycles: None,
                steps: vec![PatternStep::Hit {
                    duration: Beat::new(1, 4),
                    repeats: 1,
                    condition: Some(PassCondition {
                        passes: vec![4],
                        out_of: 4,
                    }),
                }],
            },
            TokenPorts {
                timing: TimingPorts::new(Beat::new(4, 5), Beat::from_integer(1)),
            },
            Retrigger::OnEmpty,
            Chance::default(),
        )
        .unwrap();
        engine.patterns = StepPatterns {
            accents: vec![Beat::from_integer(1), Beat::from_integer(2)],
            ..StepPatterns::default()
        };
        engine.advance(Beat::new(1, 4)).unwrap();
        engine.press(Beat::new(1, 3), 1000, 5000, 42).unwrap();
        if seek {
            engine.relocate(Beat::new(2, 3));
        } else {
            engine.pause(Beat::new(2, 3));
        }
        let events = engine.advance(Beat::new(3, 4)).unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].start);
        assert_eq!(events[0].value, 42);
        assert_eq!(events[0].accent, Beat::from_integer(2));
        engine.release(Beat::new(4, 5), 1000).unwrap();
        engine.press(Beat::new(5, 6), 1001, 5001, 43).unwrap();
        assert!(
            !engine
                .advance(Beat::new(3, 2))
                .unwrap()
                .iter()
                .any(|e| e.start)
        );
        assert_eq!(engine.advance(Beat::new(7, 4)).unwrap()[0].value, 43);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn skipped_conditions_preserve_the_audible_chance_sequence() {
    use arpeg_core::rhythm::{PassCondition, PatternStep};
    let rhythms = [
        Rhythm::Pattern {
            cycles: None,
            steps: vec![PatternStep::Hit {
                duration: Beat::new(1, 4),
                repeats: 1,
                condition: Some(PassCondition {
                    passes: vec![2],
                    out_of: 2,
                }),
            }],
        },
        Rhythm::Grid {
            step: Beat::new(1, 2),
            swing: Beat::new(1, 2),
        },
    ];
    let mut starts = Vec::new();
    for rhythm in rhythms {
        let mut engine = Arpeggiator::new(
            Bank::Held,
            Selection::Ascending,
            rhythm,
            TokenPorts {
                timing: TimingPorts::new(Beat::new(4, 5), Beat::new(1, 2)),
            },
            Retrigger::OnEmpty,
            Chance {
                probability: Beat::new(1, 2),
                seed: Some(42),
                name: "conditional".into(),
            },
        )
        .unwrap();
        engine.press(Beat::from_integer(0), 1000, 5000, 42).unwrap();
        engine.press(Beat::from_integer(0), 1001, 5001, 43).unwrap();
        starts.push(
            engine
                .advance(Beat::new(63, 4))
                .unwrap()
                .iter()
                .filter(|e| e.start)
                .map(|e| (e.at, e.value))
                .collect::<Vec<_>>(),
        );
    }
    assert!(!starts[0].is_empty());
    assert_eq!(
        starts[0],
        starts[1]
            .iter()
            .map(|(a, v)| (*a + Beat::new(1, 4), *v))
            .collect::<Vec<_>>()
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn finite_cycles_follow_transport_through_silent_passes_and_chord_edits() {
    use arpeg_core::rhythm::PatternStep;
    for rhythm in [
        Rhythm::Pattern {
            cycles: Some(2),
            steps: vec![PatternStep::Hit {
                duration: Beat::new(1, 4),
                repeats: 1,
                condition: None,
            }],
        },
        Rhythm::Euclidean {
            cycles: Some(2),
            steps: 1,
            pulses: 1,
            rotation: 0,
            step: Beat::new(1, 4),
            swing: Beat::new(1, 2),
        },
    ] {
        let mut engine = Arpeggiator::new(
            Bank::Held,
            Selection::Ascending,
            rhythm,
            TokenPorts {
                timing: TimingPorts::new(Beat::new(4, 5), Beat::from_integer(1)),
            },
            Retrigger::OnEmpty,
            Chance::default(),
        )
        .unwrap();
        engine.advance(Beat::from_integer(0)).unwrap();
        engine.press(Beat::new(1, 8), 1000, 5000, 42).unwrap();
        let events = engine.advance(Beat::new(1, 4)).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].value, 42);
        engine.release(Beat::new(1, 3), 1000).unwrap();
        engine.press(Beat::new(3, 8), 1001, 5001, 43).unwrap();
        assert!(
            !engine
                .advance(Beat::from_integer(1))
                .unwrap()
                .iter()
                .any(|e| e.start)
        );
        assert_eq!(
            engine
                .ports
                .timing
                .take_events()
                .events
                .iter()
                .filter(|e| e.port == arpeg_core::timing_ports::OutputPort::Cycle)
                .count(),
            2
        );
        engine.relocate(Beat::new(3, 4));
        assert!(engine.advance(Beat::from_integer(1)).unwrap().is_empty());
        engine.relocate(Beat::from_integer(0));
        assert_eq!(engine.advance(Beat::from_integer(0)).unwrap()[0].value, 43);
        engine.pause(Beat::from_integer(1));
        assert!(engine.advance(Beat::from_integer(2)).unwrap().is_empty());
    }
}
