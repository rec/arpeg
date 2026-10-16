use arpeg_core::{Beat, clock::ClockMode, ports::NotePort};
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn grooves_match_python_at_irregular_polling_intervals() {
    let fixture: toml::Value =
        toml::from_str(include_str!("../../../conformance/groove.toml")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let mut profile: toml::Value =
            toml::from_str(include_str!("../../../conformance/up.toml")).unwrap();
        let body = profile["body"].as_table_mut().unwrap();
        body.insert("rhythm".into(), case["rhythm"].clone());
        if let Some(patterns) = case.get("patterns") {
            body.insert("patterns".into(), patterns.clone());
        }
        for polling in [false, true] {
            let mut player = MidiPlayer::new(
                parse_profile(&toml::to_string(&profile).unwrap(), None).unwrap(),
                ClockMode::Internal,
                120,
                500_000,
            )
            .unwrap();
            for note in case["notes"].as_array().unwrap() {
                player
                    .accept(
                        0,
                        &[
                            144,
                            note[0].as_integer().unwrap() as u8,
                            note[1].as_integer().unwrap() as u8,
                        ],
                        InputSource::Both,
                    )
                    .unwrap();
            }
            let through: Beat = case["through"].as_str().unwrap().parse().unwrap();
            let end = (through * 500_000).to_integer();
            let times: Vec<_> = if polling {
                (0..end).step_by(17_000).chain([end]).collect()
            } else {
                vec![end]
            };
            let mut events = Vec::new();
            for time in times {
                player.advance(time).unwrap();
                events.extend(player.take_events().notes.into_iter().map(|e| {
                    toml::Value::Array(vec![
                        toml::Value::String(e.at.to_string()),
                        toml::Value::String(
                            if e.port == NotePort::NoteStart {
                                "on"
                            } else {
                                "off"
                            }
                            .into(),
                        ),
                        toml::Value::Integer(i64::from(e.key)),
                        toml::Value::Integer(i64::from(e.velocity)),
                    ])
                }));
            }
            assert_eq!(
                toml::Value::Array(events),
                case["events"],
                "{}",
                case["name"]
            );
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn invalid_grooves_and_combined_swing_are_rejected() {
    for rhythm in [
        "kind=\"grid\",step=\"1/4 beat\"",
        "kind=\"euclidean\",step=\"1/4 beat\",steps=3,pulses=2",
    ] {
        for groove in [
            "[]",
            "[0]",
            "[-1]",
            "[true]",
            "[0.5]",
            "[\"1/0\"]",
            "[1,2],swing=\"2/3\"",
        ] {
            let profile = format!(
                "name=\"groove\"\ntitle=\"Groove\"\n[body]\nrhythm={{{rhythm},groove={groove}}}"
            );
            assert!(parse_profile(&profile, None).is_err(), "{profile}");
        }
    }
    let profile = "name=\"groove\"\ntitle=\"Groove\"\n[body]\nrhythm={kind=\"pattern\",steps=[{kind=\"hit\",duration=\"1/4 beat\"}],groove=[1,2]}";
    assert!(parse_profile(profile, None).is_err());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn captured_gestures_fit_each_grooved_interval() {
    let profile = include_str!("../../../conformance/history-wind.toml").replace(
        "step = \"1/4 beat\"",
        "step = \"1/4 beat\", groove = [3,1,2,2]",
    );
    let mut player = MidiPlayer::new(
        parse_profile(&profile, None).unwrap(),
        ClockMode::Internal,
        120,
        500_000,
    )
    .unwrap();
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    player
        .accept(50_000, &[128, 60, 20], InputSource::Both)
        .unwrap();
    player.advance(500_000).unwrap();
    let notes: Vec<_> = player
        .take_events()
        .notes
        .iter()
        .map(|e| (e.port, e.at))
        .collect();
    assert_eq!(
        notes,
        vec![
            (NotePort::NoteStart, Beat::new(3, 8)),
            (NotePort::NoteEnd, Beat::new(19, 40)),
            (NotePort::NoteStart, Beat::new(1, 2)),
            (NotePort::NoteEnd, Beat::new(7, 10)),
            (NotePort::NoteStart, Beat::new(3, 4)),
            (NotePort::NoteEnd, Beat::new(19, 20)),
            (NotePort::NoteStart, Beat::from_integer(1)),
        ]
    );
}
