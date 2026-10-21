use arpeg_core::{Beat, clock::ClockMode, ports::NotePort};
use arpeg_midi::{
    Profile, parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn phrase_templates_substitute_live_chords_and_match_python_traces() {
    let fixture: toml::Value =
        toml::from_str(include_str!("../../../conformance/phrase-templates.toml")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        for bank in ["held", "latched"] {
            for polling in [false, true] {
                let mut document: toml::Value =
                    toml::from_str(include_str!("../../../conformance/up.toml")).unwrap();
                let body = document["body"].as_table_mut().unwrap();
                body.insert(
                    "bank".into(),
                    toml::Value::Table(toml::Table::from_iter([("kind".into(), bank.into())])),
                );
                body.insert("selection".into(), case["selection"].clone());
                for name in ["octaves", "patterns"] {
                    if let Some(value) = case.get(name) {
                        body.insert(name.into(), value.clone());
                    }
                }
                let mut player = MidiPlayer::new(
                    parse_profile(&toml::to_string(&document).unwrap(), None).unwrap(),
                    ClockMode::Internal,
                    120,
                    500_000,
                )
                .unwrap();
                for (i, key) in [60, 64, 67].into_iter().enumerate() {
                    player
                        .accept(0, &[144, key, 90 - i as u8 * 10], InputSource::Both)
                        .unwrap();
                }
                let times: Vec<_> = if polling {
                    (0..490_000).step_by(17_000).chain([490_000]).collect()
                } else {
                    vec![490_000]
                };
                let mut notes = Vec::new();
                for time in times {
                    player.advance(time).unwrap();
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
                let keys: Vec<_> = case
                    .get("held_keys")
                    .unwrap_or(&case["keys"])
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_integer().unwrap() as u8)
                    .collect();
                let beats: Vec<Beat> = if let Some(beats) = case.get("held_beats") {
                    beats
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|n| n.as_str().unwrap().parse().unwrap())
                        .collect()
                } else {
                    case["beats"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|n| n.as_str().unwrap().parse::<Beat>().unwrap() - Beat::new(1, 4))
                        .collect()
                };
                assert_eq!(
                    starts.iter().map(|n| n.key).collect::<Vec<_>>(),
                    keys,
                    "{} {bank} polling={polling}",
                    case["name"]
                );
                assert_eq!(starts.iter().map(|n| n.at).collect::<Vec<_>>(), beats);
                assert_eq!(
                    starts
                        .iter()
                        .map(|n| (n.occurrence, n.key))
                        .collect::<Vec<_>>(),
                    ends.iter()
                        .map(|n| (n.occurrence, n.key))
                        .collect::<Vec<_>>()
                );
                for note in starts {
                    let source: u8 = note.source.strip_prefix("input:").unwrap().parse().unwrap();
                    assert_eq!(note.velocity, 90 - source * 10);
                }
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn phrase_intervals_precede_scale_tuning_and_global_transposition() {
    for (changes, note, expected) in [
        (
            "selection={kind='index_pattern',indices=[0],intervals=[4]}\nscale={definition={notes='CDEFGAB'},degrees=1}\ntransposition={semitones=12}",
            60,
            77,
        ),
        (
            "selection={kind='index_pattern',indices=[0],intervals=[19]}\ntuning={source={kind='computed',notes_per_octave=19,octave_ratio='2'},root_note=69,root_frequency='440'}\nbend_range=2",
            69,
            81,
        ),
        (
            "selection={kind='index_pattern',indices=[0],intervals=[100]}\ntransposition={boundary='fold'}",
            60,
            124,
        ),
    ] {
        let mut player =
            MidiPlayer::new(profile(changes), ClockMode::Internal, 120, 500_000).unwrap();
        player
            .accept(0, &[144, note, 90], InputSource::Both)
            .unwrap();
        player.advance(0).unwrap();
        assert_eq!(player.take_events().notes[0].key, expected);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn repeated_attacks_and_ties_keep_the_selected_phrase_interval() {
    let mut player = MidiPlayer::new(profile("selection={kind='index_pattern',indices=[0,1],intervals=[12,-12]}\nrhythm={kind='pattern',steps=[{kind='hit',duration='1/2 beat',repeats=2},{kind='tie',duration='1/4 beat'}]}"), ClockMode::Internal, 120, 500_000).unwrap();
    for (key, velocity) in [(60, 90), (64, 80)] {
        player
            .accept(0, &[144, key, velocity], InputSource::Both)
            .unwrap();
    }
    player.advance(0).unwrap();
    let mut notes = player.take_events().notes;
    player
        .accept(62_500, &[144, 48, 70], InputSource::Both)
        .unwrap();
    notes.extend(player.take_events().notes);
    player.advance(500_000).unwrap();
    notes.extend(player.take_events().notes);
    assert_eq!(
        notes
            .iter()
            .filter(|n| n.port == NotePort::NoteStart)
            .map(|n| (n.at, n.key, n.velocity))
            .collect::<Vec<_>>(),
        [
            (Beat::from_integer(0), 72, 90),
            (Beat::new(1, 4), 72, 90),
            (Beat::new(3, 4), 48, 90),
            (Beat::from_integer(1), 48, 90)
        ]
    );
    assert_eq!(
        notes
            .iter()
            .filter(|n| n.port == NotePort::NoteEnd)
            .map(|n| (n.at, n.key))
            .collect::<Vec<_>>(),
        [
            (Beat::new(1, 5), 72),
            (Beat::new(7, 10), 72),
            (Beat::new(19, 20), 48)
        ]
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn phrase_intervals_require_one_signed_integer_per_index() {
    for intervals in [
        "[]",
        "[0]",
        "[0,1,2]",
        "[true,0]",
        "[0.5,0]",
        "['1',0]",
        "[9223372036854775808,0]",
    ] {
        let source = format!(
            "name='invalid'\n[body]\nrhythm={{kind='grid',step='1/4 beat'}}\nselection={{kind='index_pattern',indices=[0,1],intervals={intervals}}}"
        );
        assert!(parse_profile(&source, None).is_err(), "{intervals}");
    }
}

fn profile(changes: &str) -> Profile {
    let mut source: toml::Value =
        toml::from_str(include_str!("../../../conformance/up.toml")).unwrap();
    let changes: toml::Table = toml::from_str(changes).unwrap();
    source["body"].as_table_mut().unwrap().extend(changes);
    parse_profile(&toml::to_string(&source).unwrap(), None).unwrap()
}
