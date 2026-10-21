use arpeg_core::{
    Beat, IntervalEnvelope, RankRange, Selection, Transition,
    bank::{CaptureBank, CaptureMode},
    capture::{MidiEvent, Profile},
    chance::Chance,
    clock::ClockMode,
    history::HistoryArpeggiator,
    ports::NotePort,
};
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn captured_selectors_preserve_gestures_and_match_python_traces() {
    let fixture: toml::Value =
        toml::from_str(include_str!("../../../conformance/captured-selectors.toml")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        for phrase in [false, true] {
            for polling in [false, true] {
                let source = if phrase {
                    include_str!("../../../conformance/phrase-wind.toml")
                } else {
                    include_str!("../../../conformance/history-wind.toml")
                };
                let mut profile: toml::Value = toml::from_str(source).unwrap();
                let body = profile["body"].as_table_mut().unwrap();
                body.insert("selection".into(), case["selection"].clone());
                body.insert("seed".into(), 42.into());
                body.insert("retrigger".into(), "on_empty".into());
                for name in ["octaves", "patterns"] {
                    if let Some(value) = case.get(name) {
                        body.insert(name.into(), value.clone());
                    }
                }
                let mut player = MidiPlayer::new(
                    parse_profile(&toml::to_string(&profile).unwrap(), None).unwrap(),
                    ClockMode::Internal,
                    120,
                    500_000,
                )
                .unwrap();
                if phrase {
                    player.capture(0, "record").unwrap();
                }
                for (i, key) in [60, 64, 67].into_iter().enumerate() {
                    let at = i as i64 * 20_000;
                    player
                        .accept(at, &[176, 2, (i as u8 + 1) * 10], InputSource::Both)
                        .unwrap();
                    player
                        .accept(
                            at + 1_000,
                            &[144, key, 90 - i as u8 * 10],
                            InputSource::Both,
                        )
                        .unwrap();
                    player
                        .accept(
                            at + 10_000,
                            &[128, key, (i as u8 + 1) * 11],
                            InputSource::Both,
                        )
                        .unwrap();
                }
                if phrase {
                    player.capture(60_000, "commit").unwrap();
                }
                player.take_events();
                let times: Vec<_> = if polling {
                    (60_000..600_000).step_by(17_000).chain([600_000]).collect()
                } else {
                    vec![600_000]
                };
                let mut messages = Vec::new();
                let mut notes = Vec::new();
                for time in times {
                    messages.extend(player.advance(time).unwrap());
                    notes.extend(player.take_events().notes);
                }
                let starts: Vec<_> = notes
                    .iter()
                    .filter(|n| n.port == NotePort::NoteStart)
                    .collect();
                let ends: Vec<_> = notes
                    .iter()
                    .filter(|n| n.port == NotePort::NoteEnd)
                    .collect();
                let expected_keys: Vec<_> = case["keys"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_integer().unwrap() as u8)
                    .collect();
                let expected_beats: Vec<Beat> = case["beats"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_str().unwrap().parse().unwrap())
                    .collect();
                assert_eq!(
                    starts.iter().map(|n| n.key).collect::<Vec<_>>(),
                    expected_keys,
                    "{} phrase={phrase} polling={polling}",
                    case["name"]
                );
                assert_eq!(
                    starts.iter().map(|n| n.at).collect::<Vec<_>>(),
                    expected_beats
                );
                assert_eq!(
                    starts
                        .iter()
                        .map(|n| (n.occurrence, &n.source))
                        .collect::<Vec<_>>(),
                    ends.iter()
                        .map(|n| (n.occurrence, &n.source))
                        .collect::<Vec<_>>()
                );
                for (start, end) in starts.iter().zip(&ends) {
                    let (attack, _, release) = gesture(start.key);
                    assert_eq!(start.velocity, attack);
                    assert_eq!(end.velocity, release);
                }
                for (i, message) in messages.iter().enumerate() {
                    if message[0] == 144 {
                        assert_eq!(messages[i - 1], [176, 2, gesture(message[1]).1]);
                    }
                }
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn captured_selection_preserves_identity_or_retriggers_on_edits() {
    let chance = Chance {
        seed: Some(42),
        ..Chance::default()
    };
    for (selection, after_edit) in selections() {
        for retrigger in [false, true] {
            let mut bank = CaptureBank::new(
                CaptureMode::Phrase,
                selection.clone(),
                retrigger,
                Profile::default(),
            )
            .unwrap();
            capture_notes(&mut bank, "chord", &[60, 64, 67], false);
            let c = bank.select_step(&chance, 0).unwrap().unwrap();
            let e = bank.select_step(&chance, 1).unwrap().unwrap();
            assert_eq!(c.note_id, "note-0");
            assert_eq!(e.note_id, "note-1");
            capture_notes(&mut bank, "added", &[48], true);
            let selected = bank.select_step(&chance, 2).unwrap().unwrap();
            let phrase = bank.source(&selected.capture_id).unwrap();
            assert_eq!(
                phrase
                    .notes
                    .iter()
                    .find(|n| n.note_id == selected.note_id)
                    .unwrap()
                    .key,
                if retrigger { 48 } else { after_edit }
            );
            bank.undo().unwrap();
            assert_eq!(bank.select_step(&chance, 3).unwrap(), Some(c));
            bank.clear();
            assert_eq!(bank.select_step(&chance, 4).unwrap(), None);
            capture_notes(&mut bank, "new", &[72], false);
            assert_eq!(
                bank.select_step(&chance, 5).unwrap().unwrap().capture_id,
                "new"
            );
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn pause_preserves_captured_selection_and_seek_restarts_it() {
    for (selection, _) in selections() {
        let mut engine = HistoryArpeggiator::new(
            CaptureMode::Phrase,
            selection,
            false,
            Beat::new(1, 4),
            Beat::new(4, 5),
            Profile::default(),
            false,
        )
        .unwrap();
        engine.chance.seed = Some(42);
        capture_notes(&mut engine.bank, "chord", &[60, 64, 67], false);
        let first = engine.advance(Beat::from_integer(0), 300_000).unwrap();
        assert_eq!(first.iter().find(|e| e.data[0] == 144).unwrap().data[1], 60);
        engine.pause(Beat::new(1, 8), 350_000);
        let second = engine.advance(Beat::new(1, 4), 400_000).unwrap();
        assert_eq!(
            second.iter().find(|e| e.data[0] == 144).unwrap().data[1],
            64
        );
        engine.relocate(Beat::from_integer(0), 450_000);
        let restarted = engine.advance(Beat::from_integer(0), 450_000).unwrap();
        assert_eq!(
            restarted.iter().find(|e| e.data[0] == 144).unwrap().data[1],
            60
        );
    }
}

fn selections() -> Vec<(Selection, u8)> {
    vec![
        (
            Selection::Transition(Transition {
                weights: vec![vec![0, 1, 0], vec![0, 0, 1], vec![1, 0, 0]],
                missing_weight: 0,
                start_random: false,
                uniform_when_blocked: false,
            }),
            48,
        ),
        (
            Selection::IntervalEnvelope(IntervalEnvelope {
                ranges: vec![RankRange { min: 1, max: 1 }],
                rest_outside: false,
            }),
            67,
        ),
        (
            Selection::IndexPattern {
                indices: vec![0, 1, 2],
                rest_outside: false,
            },
            64,
        ),
    ]
}

fn capture_notes(bank: &mut CaptureBank, id: &str, keys: &[u8], overdub: bool) {
    bank.record(id, Profile::default()).unwrap();
    for (i, key) in keys.iter().enumerate() {
        let tick = i as i64 * 100_000;
        bank.accept(MidiEvent {
            tick,
            ordinal: 0,
            data: vec![144, *key, 90],
        })
        .unwrap();
        bank.accept(MidiEvent {
            tick: tick + 50_000,
            ordinal: 0,
            data: vec![128, *key, 11],
        })
        .unwrap();
    }
    bank.commit(keys.len() as i64 * 100_000, overdub).unwrap();
}

fn gesture(key: u8) -> (u8, u8, u8) {
    match key {
        60 | 72 => (90, 10, 11),
        64 => (80, 20, 22),
        67 => (70, 30, 33),
        _ => panic!("unexpected selected key {key}"),
    }
}
