use arpeg_core::{Beat, clock::ClockMode, ports::NotePort};
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn accent_and_gate_patterns_match_python_at_irregular_polling_intervals() {
    let fixture: toml::Value =
        toml::from_str(include_str!("../../../conformance/step-patterns.toml")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let mut profile: toml::Value =
            toml::from_str(include_str!("../../../conformance/up.toml")).unwrap();
        let body = profile["body"].as_table_mut().unwrap();
        body.insert("rhythm".into(), case["rhythm"].clone());
        body.insert("patterns".into(), case["patterns"].clone());
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
fn invalid_pattern_values_and_file_rendering_are_rejected() {
    for pattern in [
        "accents=[]",
        "gates=[]",
        "accents=[-1]",
        "gates=[-1]",
        "accents=[0.5]",
        "gates=[true]",
        "accents=[\"1/0\"]",
        "unknown=[1]",
    ] {
        let profile = format!(
            "name=\"patterns\"\ntitle=\"Patterns\"\n[body]\nrhythm={{kind=\"grid\",step=\"1/4 beat\"}}\npatterns={{{pattern}}}"
        );
        assert!(parse_profile(&profile, None).is_err(), "{pattern}");
    }
    let profile = include_str!("../../../conformance/up.toml").to_owned()
        + "\n[body.patterns]\naccents=[1,0]\n";
    assert!(
        arpeg_midi::render_file(&profile, &[], None)
            .unwrap_err()
            .contains("require live input")
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn history_preserves_recorded_expression_and_release_velocity() {
    let profile = include_str!("../../../conformance/history-wind.toml").to_owned()
        + "\n[body.patterns]\naccents=[\"0\",\"1/2\"]\ngates=[\"1/2\"]\n";
    let mut player = MidiPlayer::new(
        parse_profile(&profile, None).unwrap(),
        ClockMode::Internal,
        120,
        500_000,
    )
    .unwrap();
    for (at, data) in [
        (0, [176, 2, 30]),
        (0, [144, 60, 81]),
        (50_000, [128, 60, 20]),
    ] {
        player.accept(at, &data, InputSource::Both).unwrap();
    }
    assert_eq!(
        player.advance(225_000).unwrap(),
        [vec![176, 2, 30], vec![144, 60, 41], vec![128, 60, 20]]
    );
}
