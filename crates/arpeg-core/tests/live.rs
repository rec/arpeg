use arpeg_core::{
    Bank, Beat, Selection,
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
        beat(1, 4),
        beat(4, 5),
        Retrigger::OnEmpty,
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
        beat(1, 4),
        beat(1, 1),
        Retrigger::OnEmpty,
    )
    .unwrap();
    let mut b = LiveArpeggiator::new(
        Bank::Held,
        Selection::Ascending,
        beat(1, 4),
        beat(1, 1),
        Retrigger::OnEmpty,
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
        beat(1, 4),
        beat(1, 1),
        Retrigger::OnEmpty,
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
    let fixture: Value =
        serde_json::from_str(include_str!("../../../conformance/live-classic.json")).unwrap();
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
        let mut arp = LiveArpeggiator::new(
            bank,
            Selection::Ascending,
            beat(1, 4),
            beat(4, 5),
            retrigger,
        )
        .unwrap();
        let mut events = Vec::new();
        for action in case["actions"].as_array().unwrap() {
            let at = ratio(action[1].as_str().unwrap());
            let result = match action[0].as_str().unwrap() {
                "on" => arp.note_on(
                    at,
                    action[2].as_u64().unwrap() as u8,
                    action[3].as_u64().unwrap() as u8,
                ),
                "off" => arp.note_off(at, action[2].as_u64().unwrap() as u8),
                "advance" => arp.advance(at),
                "clear" => arp.clear(at),
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
                    OutputKind::NoteOff { id, source_id, key } => ("off", id, source_id, key, 0),
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

fn ratio(text: &str) -> Beat {
    match text.split_once('/') {
        Some((numerator, denominator)) => {
            Beat::new(numerator.parse().unwrap(), denominator.parse().unwrap())
        }
        None => Beat::from_integer(text.parse().unwrap()),
    }
}
