use arpeg_core::capture::{MidiCapture, MidiEvent, Overlap, Profile, Timebase};
use arpeg_core::gesture::{self, OverlapPolicy, Placement, Tick, Timing};
use serde_json::Value;

fn timebase() -> Timebase {
    Timebase {
        name: "milliseconds".into(),
        rate_numerator: 1000,
        rate_denominator: 1,
    }
}

fn fixture(name: &str, profile: Profile) -> (Value, arpeg_core::capture::CapturedPhrase) {
    let text = match name {
        "wind-breath" => include_str!("../../../conformance/wind-breath.json"),
        "gap" => include_str!("../../../conformance/gap.json"),
        _ => panic!("unknown capture fixture: {name}"),
    };
    let source: Value = serde_json::from_str(text).unwrap();
    let mut capture =
        MidiCapture::new(source["capture_id"].as_str().unwrap(), timebase(), profile).unwrap();
    let ids: Vec<_> = source["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|note| note["note_id"].as_str().unwrap())
        .collect();
    let mut next_id = 0;
    for event in source["events"].as_array().unwrap() {
        let data: Vec<_> = event["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|byte| byte.as_u64().unwrap() as u8)
            .collect();
        let onset = data.len() == 3 && data[0] & 0xf0 == 0x90 && data[2] > 0;
        let note_id = onset.then(|| {
            let id = ids[next_id];
            next_id += 1;
            id
        });
        capture
            .accept(
                MidiEvent {
                    tick: event["tick"].as_i64().unwrap(),
                    ordinal: event["ordinal"].as_u64().unwrap() as u32,
                    data,
                },
                note_id,
            )
            .unwrap();
    }
    let phrase = capture
        .finish(source["end_tick"].as_i64().unwrap())
        .unwrap();
    (source, phrase)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn wind_and_gap_fixtures_preserve_ledger_and_note_boundaries() {
    for (name, profile) in [
        (
            "wind-breath",
            Profile {
                channel: 2,
                ..Profile::default()
            },
        ),
        (
            "gap",
            Profile {
                track_bend: false,
                ..Profile::default()
            },
        ),
    ] {
        let (source, phrase) = fixture(name, profile);
        assert_eq!(phrase.timebase, timebase());
        assert_eq!(
            phrase.timebase.name,
            source["timebase"]["name"].as_str().unwrap()
        );
        assert_eq!(
            phrase.timebase.rate_numerator,
            source["timebase"]["rate"]["numerator"].as_i64().unwrap()
        );
        assert_eq!(
            phrase.events.len(),
            source["events"].as_array().unwrap().len()
        );
        for (actual, expected) in phrase
            .events
            .iter()
            .zip(source["events"].as_array().unwrap())
        {
            assert_eq!(actual.tick, expected["tick"].as_i64().unwrap());
            assert_eq!(actual.ordinal, expected["ordinal"].as_u64().unwrap() as u32);
            let bytes: Vec<_> = expected["data"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u8)
                .collect();
            assert_eq!(actual.data, bytes);
        }
        assert_eq!(
            phrase.notes.len(),
            source["notes"].as_array().unwrap().len()
        );
        let prefix: Vec<_> = source["prefix_events"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .map(|value| value.as_u64().unwrap() as usize)
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(phrase.prefix_events, prefix);
        for (actual, expected) in phrase.notes.iter().zip(source["notes"].as_array().unwrap()) {
            assert_eq!(actual.capture_id, phrase.capture_id);
            assert_eq!(actual.note_id, expected["note_id"].as_str().unwrap());
            assert_eq!(actual.onset_tick, expected["onset_tick"].as_i64().unwrap());
            assert_eq!(
                actual.gate_end_tick,
                expected["gate_end_tick"].as_i64().unwrap()
            );
            assert_eq!(
                actual.cell_end_tick,
                expected["cell_end_tick"].as_i64().unwrap()
            );
            assert_eq!(
                actual.onset_event,
                expected["onset_event"].as_u64().unwrap() as usize
            );
            assert_eq!(
                actual.release_event,
                expected["release_event"].as_u64().map(|v| v as usize)
            );
            for (lane, state) in &actual.entry_state {
                let expected = &expected["entry_state"][lane];
                assert_eq!(
                    state.source_event,
                    expected["source_event"].as_u64().map(|v| v as usize)
                );
                match (state.value, expected["value"].as_f64()) {
                    (Some(actual), Some(expected)) => assert!((actual - expected).abs() < 1e-15),
                    (actual, expected) => assert_eq!(actual, expected),
                }
            }
            for (actual, field) in [
                (&actual.expression_events, "expression_events"),
                (&actual.following_events, "following_events"),
            ] {
                let expected: Vec<_> = expected[field]
                    .as_array()
                    .map(|values| {
                        values
                            .iter()
                            .map(|value| value.as_u64().unwrap() as usize)
                            .collect()
                    })
                    .unwrap_or_default();
                assert_eq!(*actual, expected);
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn legato_handoff_preserves_velocity_zero_wire_release() {
    let mut capture = MidiCapture::new("legato", timebase(), Profile::default()).unwrap();
    for (tick, data) in [
        (0, vec![144, 60, 100]),
        (10, vec![176, 2, 70]),
        (20, vec![144, 64, 90]),
        (30, vec![144, 64, 0]),
    ] {
        capture
            .accept(
                MidiEvent {
                    tick,
                    ordinal: 0,
                    data,
                },
                None,
            )
            .unwrap();
    }
    let phrase = capture.finish(40).unwrap();
    assert_eq!(phrase.notes[0].gate_end_tick, 20);
    assert_eq!(phrase.notes[0].release_event, None);
    assert_eq!(phrase.notes[1].entry_state["breath"].source_event, Some(1));
    assert_eq!(phrase.notes[1].release_event, Some(3));
    assert_eq!(phrase.events[3].data, [144, 64, 0]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn independent_overlap_keeps_both_gates_and_shared_expression() {
    let mut capture = MidiCapture::new(
        "poly",
        timebase(),
        Profile {
            overlap: Overlap::Independent,
            ..Profile::default()
        },
    )
    .unwrap();
    for (tick, data) in [
        (0, vec![144, 60, 90]),
        (10, vec![144, 64, 90]),
        (20, vec![176, 2, 80]),
        (30, vec![128, 60, 0]),
        (40, vec![128, 64, 0]),
    ] {
        capture
            .accept(
                MidiEvent {
                    tick,
                    ordinal: 0,
                    data,
                },
                None,
            )
            .unwrap();
    }
    let phrase = capture.finish(50).unwrap();
    assert_eq!(phrase.notes[0].gate_end_tick, 30);
    assert_eq!(phrase.notes[1].gate_end_tick, 40);
    assert_eq!(phrase.notes[0].expression_events, [2]);
    assert_eq!(phrase.notes[1].expression_events, [2]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn completed_note_waits_for_declared_tail() {
    let mut capture = MidiCapture::new(
        "history",
        timebase(),
        Profile {
            tail_ticks: 20,
            ..Profile::default()
        },
    )
    .unwrap();
    capture
        .accept(
            MidiEvent {
                tick: 0,
                ordinal: 0,
                data: vec![144, 60, 90],
            },
            None,
        )
        .unwrap();
    capture
        .accept(
            MidiEvent {
                tick: 100,
                ordinal: 0,
                data: vec![128, 60, 0],
            },
            None,
        )
        .unwrap();
    assert!(capture.advance(119).unwrap().is_empty());
    let ready = capture.advance(120).unwrap();
    assert_eq!(ready[0].cell_end_tick, 120);
    assert!(capture.advance(130).unwrap().is_empty());
    assert!(
        capture
            .accept(
                MidiEvent {
                    tick: 125,
                    ordinal: 0,
                    data: vec![176, 2, 60]
                },
                None
            )
            .is_err()
    );
    assert!(capture.advance(129).is_err());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reordered_wind_gestures_restore_entry_state_and_keep_local_timing() {
    let (_, phrase) = fixture(
        "wind-breath",
        Profile {
            channel: 2,
            ..Profile::default()
        },
    );
    let events = gesture::reorder(&phrase, &["e", "c"], &[0]).unwrap();
    let trace: Vec<_> = events
        .iter()
        .map(|event| (*event.at.numer(), event.data.clone(), event.source_event))
        .collect();
    assert_eq!(
        trace,
        [
            (0, vec![224, 64, 81], Some(4)),
            (0, vec![176, 2, 13], Some(6)),
            (0, vec![144, 64, 90], Some(7)),
            (15, vec![176, 2, 83], Some(8)),
            (180, vec![128, 64, 16], Some(9)),
            (180, vec![176, 2, 0], Some(0)),
            (180, vec![144, 60, 100], Some(1)),
            (188, vec![176, 2, 38], Some(2)),
            (220, vec![176, 2, 102], Some(3)),
            (270, vec![224, 64, 81], Some(4)),
            (360, vec![128, 60, 20], Some(5)),
        ]
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn overlapping_gestures_require_separate_channels() {
    let (_, phrase) = fixture(
        "wind-breath",
        Profile {
            channel: 2,
            ..Profile::default()
        },
    );
    let placements = [
        Placement {
            note_id: "c".into(),
            onset: Tick::from_integer(0),
            gate: None,
        },
        Placement {
            note_id: "e".into(),
            onset: Tick::from_integer(50),
            gate: None,
        },
    ];
    assert!(
        gesture::render(
            &phrase,
            &placements,
            &[0],
            Timing::Original,
            OverlapPolicy::Reject
        )
        .is_err()
    );
    let events = gesture::render(
        &phrase,
        &placements,
        &[0, 1],
        Timing::Original,
        OverlapPolicy::Reject,
    )
    .unwrap();
    let onsets: Vec<_> = events
        .iter()
        .filter(|event| event.data[0] & 0xf0 == 0x90)
        .map(|event| (event.at, event.data.clone()))
        .collect();
    assert_eq!(
        onsets,
        [
            (Tick::from_integer(0), vec![144, 60, 100]),
            (Tick::from_integer(50), vec![145, 64, 90]),
        ]
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn monophonic_handoff_releases_the_old_note_before_new_state() {
    let (_, phrase) = fixture(
        "wind-breath",
        Profile {
            channel: 2,
            ..Profile::default()
        },
    );
    let placements = [
        Placement {
            note_id: "c".into(),
            onset: Tick::from_integer(0),
            gate: None,
        },
        Placement {
            note_id: "e".into(),
            onset: Tick::from_integer(50),
            gate: None,
        },
    ];
    let events = gesture::render(
        &phrase,
        &placements,
        &[0],
        Timing::Original,
        OverlapPolicy::Handoff,
    )
    .unwrap();
    let at_handoff: Vec<_> = events
        .iter()
        .filter(|event| event.at == Tick::from_integer(50))
        .map(|event| (event.data.clone(), event.source_event))
        .collect();
    assert_eq!(
        at_handoff,
        [
            (vec![128, 60, 0], None),
            (vec![224, 64, 81], Some(4)),
            (vec![176, 2, 13], Some(6)),
            (vec![144, 64, 90], Some(7)),
        ]
    );
}
