use arpeg_core::chance::Chance;
use arpeg_core::rhythm::{PatternStep, Rhythm};
use arpeg_core::{
    Bank, Beat, Selection, Walk,
    live::{LiveArpeggiator, OutputEvent, OutputKind, Retrigger},
};
use serde_json::{Value, json};

fn beat(n: i64, d: i64) -> Beat {
    Beat::new(n, d)
}

#[test]
fn live_notes_follow_input_and_release_when_bank_empties() {
    let mut arp = LiveArpeggiator::new(
        Bank::Held,
        Selection::Ascending,
        Rhythm::Grid { step: beat(1, 4) },
        beat(4, 5),
        Retrigger::OnEmpty,
        Chance::default(),
    )
    .unwrap();
    assert_eq!(arp.note_on(beat(0, 1), 60, 100).unwrap(), []);
    assert_eq!(
        arp.advance(beat(0, 1)).unwrap(),
        [OutputEvent {
            at: beat(0, 1),
            kind: OutputKind::NoteOn {
                id: 0,
                source_id: 0,
                key: 60,
                velocity: 100
            }
        }]
    );
    assert_eq!(arp.note_on(beat(1, 8), 64, 90).unwrap(), []);
    assert_eq!(
        arp.advance(beat(1, 4)).unwrap(),
        [
            OutputEvent {
                at: beat(1, 5),
                kind: OutputKind::NoteOff {
                    id: 0,
                    source_id: 0,
                    key: 60
                }
            },
            OutputEvent {
                at: beat(1, 4),
                kind: OutputKind::NoteOn {
                    id: 1,
                    source_id: 1,
                    key: 64,
                    velocity: 90
                }
            },
        ]
    );
    assert_eq!(arp.note_off(beat(3, 10), 60).unwrap(), []);
    assert_eq!(
        arp.note_off(beat(7, 20), 64).unwrap(),
        [OutputEvent {
            at: beat(7, 20),
            kind: OutputKind::NoteOff {
                id: 1,
                source_id: 1,
                key: 64
            }
        }]
    );
    assert_eq!(arp.advance(beat(1, 2)).unwrap(), []);
}

#[test]
fn live_step_order_is_independent_of_poll_intervals() {
    let mut a = LiveArpeggiator::new(
        Bank::Held,
        Selection::Ascending,
        Rhythm::Grid { step: beat(1, 4) },
        beat(1, 1),
        Retrigger::OnEmpty,
        Chance::default(),
    )
    .unwrap();
    let mut b = LiveArpeggiator::new(
        Bank::Held,
        Selection::Ascending,
        Rhythm::Grid { step: beat(1, 4) },
        beat(1, 1),
        Retrigger::OnEmpty,
        Chance::default(),
    )
    .unwrap();
    a.note_on(beat(0, 1), 60, 100).unwrap();
    b.note_on(beat(0, 1), 60, 100).unwrap();
    a.advance(beat(0, 1)).unwrap();
    b.advance(beat(0, 1)).unwrap();
    let one = a.advance(beat(1, 1)).unwrap();
    let mut pieces = Vec::new();
    for i in 1..=8 {
        pieces.extend(b.advance(beat(i, 8)).unwrap());
    }
    assert_eq!(one, pieces);
}

#[test]
fn simultaneous_note_ons_join_the_first_step() {
    let mut arp = LiveArpeggiator::new(
        Bank::Held,
        Selection::Ascending,
        Rhythm::Grid { step: beat(1, 4) },
        beat(1, 1),
        Retrigger::OnEmpty,
        Chance::default(),
    )
    .unwrap();
    arp.note_on(beat(0, 1), 64, 90).unwrap();
    arp.note_on(beat(0, 1), 60, 100).unwrap();
    assert_eq!(
        arp.advance(beat(0, 1)).unwrap(),
        [OutputEvent {
            at: beat(0, 1),
            kind: OutputKind::NoteOn {
                id: 0,
                source_id: 1,
                key: 60,
                velocity: 100
            }
        }]
    );
}

#[test]
fn live_classic_matches_shared_python_rust_traces() {
    for text in [
        include_str!("../../../conformance/live-classic.json"),
        include_str!("../../../conformance/euclidean.json"),
        include_str!("../../../conformance/custom-steps.json"),
        include_str!("../../../conformance/chance-walk.json"),
        include_str!("../../../conformance/alternating.json"),
        include_str!("../../../conformance/center-edge.json"),
        include_str!("../../../conformance/index-pattern.json"),
    ] {
        let fixture: Value = serde_json::from_str(text).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let bank = match case["bank"].as_str().unwrap() {
                "held" => Bank::Held,
                "replace" => Bank::LatchedReplace,
                "add" => Bank::LatchedAdd,
                "toggle" => Bank::LatchedToggle,
                _ => panic!("unsupported fixture bank"),
            };
            let retrigger = match case["retrigger"].as_str().unwrap() {
                "on_empty" => Retrigger::OnEmpty,
                "bank_edit" => Retrigger::BankEdit,
                _ => panic!("unsupported fixture retrigger"),
            };
            let rhythm = match case.get("rhythm") {
                Some(rhythm) if rhythm["kind"] == "pattern" => Rhythm::Pattern {
                    steps: rhythm["steps"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|step| {
                            let duration = ratio(
                                step["duration"]
                                    .as_str()
                                    .unwrap()
                                    .strip_suffix(" beat")
                                    .unwrap(),
                            );
                            match step["kind"].as_str().unwrap() {
                                "hit" => PatternStep::Hit {
                                    duration,
                                    repeats: step
                                        .get("repeats")
                                        .map_or(1, |v| v.as_u64().unwrap() as usize),
                                },
                                "rest" => PatternStep::Rest { duration },
                                "tie" => PatternStep::Tie { duration },
                                _ => panic!("unsupported step"),
                            }
                        })
                        .collect(),
                },
                Some(rhythm) => Rhythm::Euclidean {
                    step: ratio(
                        rhythm["step"]
                            .as_str()
                            .unwrap()
                            .strip_suffix(" beat")
                            .unwrap(),
                    ),
                    steps: rhythm["steps"].as_i64().unwrap(),
                    pulses: rhythm["pulses"].as_i64().unwrap(),
                    rotation: rhythm["rotation"].as_i64().unwrap(),
                },
                None => Rhythm::Grid { step: beat(1, 4) },
            };
            let selection = match case.get("selection") {
                Some(s) if s["kind"] == "index_pattern" => Selection::IndexPattern {
                    indices: s["indices"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|i| i.as_u64().unwrap())
                        .collect(),
                    rest_outside: s["boundary"] == "rest",
                },
                Some(s) if s["kind"] == "inside_out" => Selection::InsideOut,
                Some(s) if s["kind"] == "outside_in" => Selection::OutsideIn,
                Some(s) if s["kind"] == "alternating" => Selection::Alternating {
                    repeat_endpoints: s["repeat_endpoints"].as_bool().unwrap_or(false),
                },
                Some(s) => Selection::Walk(Walk {
                    moves: serde_json::from_value(s["moves"].clone()).unwrap(),
                    weights: serde_json::from_value(s["weights"].clone()).unwrap(),
                    start_move: s["start"] == "move",
                    keep_rank: s["on_remove"] == "rank",
                }),
                None => Selection::Ascending,
            };
            let chance = Chance {
                probability: case
                    .get("probability")
                    .map_or(beat(1, 1), |v| ratio(v.as_str().unwrap())),
                seed: case.get("seed").and_then(Value::as_i64),
                name: case
                    .get("profile_name")
                    .map_or("up", |v| v.as_str().unwrap())
                    .to_owned(),
            };
            for polling in [false, true] {
                let mut arp = LiveArpeggiator::new(
                    bank,
                    selection.clone(),
                    rhythm.clone(),
                    case.get("gate")
                        .map_or(beat(4, 5), |v| ratio(v.as_str().unwrap())),
                    retrigger,
                    chance.clone(),
                )
                .unwrap();
                let mut events = Vec::new();
                let mut last = beat(0, 1);
                for action in case["actions"].as_array().unwrap() {
                    let at = ratio(action[1].as_str().unwrap());
                    if polling {
                        while last + beat(1, 17) < at {
                            last += beat(1, 17);
                            events.extend(arp.advance(last).unwrap());
                        }
                    }
                    last = at;
                    let result = match action[0].as_str().unwrap() {
                        "on" => arp.note_on(
                            at,
                            action[2].as_u64().unwrap() as u8,
                            action[3].as_u64().unwrap() as u8,
                        ),
                        "off" => arp.note_off(at, action[2].as_u64().unwrap() as u8),
                        "advance" => arp.advance(at),
                        "clear" => arp.clear(at),
                        "stop" => arp.stop(at),
                        _ => panic!("unsupported fixture action"),
                    };
                    events.extend(result.unwrap());
                }
                let actual: Vec<Value> = events
                    .into_iter()
                    .map(|event| {
                        let (kind, id, source_id, key, velocity) = match event.kind {
                            OutputKind::NoteOn {
                                id,
                                source_id,
                                key,
                                velocity,
                            } => ("on", id, source_id, key, velocity),
                            OutputKind::NoteOff { id, source_id, key } => {
                                ("off", id, source_id, key, 0)
                            }
                        };
                        json!([event.at.to_string(), kind, id, source_id, key, velocity])
                    })
                    .collect();
                assert_eq!(
                    actual,
                    *case["expected"].as_array().unwrap(),
                    "{}",
                    case["name"]
                );
            }
        }
    }
}

fn ratio(text: &str) -> Beat {
    match text.split_once('/') {
        Some((numerator, denominator)) => {
            Beat::new(numerator.parse().unwrap(), denominator.parse().unwrap())
        }
        None => Beat::from_integer(text.parse().unwrap()),
    }
}
