use arpeg_core::{clock::ClockMode, ports::NotePort};
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn swung_lifecycle_and_cycles_match_python_at_irregular_polling_intervals() {
    let fixture: toml::Value =
        toml::from_str(include_str!("../../../conformance/swing.toml")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let mut profile: toml::Value =
            toml::from_str(include_str!("../../../conformance/up.toml")).unwrap();
        let body = profile["body"].as_table_mut().unwrap();
        body.insert("rhythm".into(), case["rhythm"].clone());
        body.insert("gate".into(), case["gate"].clone());
        for polling in [false, true] {
            let mut player = MidiPlayer::new(
                parse_profile(&toml::to_string(&profile).unwrap(), None).unwrap(),
                ClockMode::Internal,
                120,
                500_000,
            )
            .unwrap();
            player
                .accept(0, &[144, 60, 100], InputSource::Both)
                .unwrap();
            let through: arpeg_core::Beat = case["through"].as_str().unwrap().parse().unwrap();
            let end = (through * 500_000).to_integer();
            let mut notes = Vec::new();
            let mut cycles = Vec::new();
            let times: Vec<_> = if polling {
                (0..end).step_by(17_000).chain([end]).collect()
            } else {
                vec![end]
            };
            for at in times {
                player.advance(at).unwrap();
                let batch = player.take_events();
                notes.extend(batch.notes.into_iter().map(|n| {
                    vec![
                        toml::Value::String(n.at.to_string()),
                        toml::Value::String(
                            if n.port == NotePort::NoteStart {
                                "on"
                            } else {
                                "off"
                            }
                            .into(),
                        ),
                    ]
                }));
                cycles.extend(
                    batch
                        .events
                        .into_iter()
                        .filter(|e| e.port == arpeg_core::ports::OutputPort::Cycle)
                        .map(|e| toml::Value::String(e.at.to_string())),
                );
            }
            let actual: Vec<_> = notes.into_iter().map(toml::Value::Array).collect();
            assert_eq!(
                toml::Value::Array(actual),
                case["events"],
                "{}",
                case["name"]
            );
            assert_eq!(
                toml::Value::Array(cycles),
                case["cycles"],
                "{}",
                case["name"]
            );
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn custom_patterns_and_invalid_swing_are_rejected() {
    for rhythm in [
        "kind=\"grid\",step=\"1/4 beat\",swing=\"1/3\"",
        "kind=\"grid\",step=\"1/4 beat\",swing=\"1\"",
        "kind=\"euclidean\",steps=3,pulses=2,step=\"1/4 beat\",swing=\"3/2\"",
        "kind=\"pattern\",steps=[{kind=\"hit\",duration=\"1 beat\"}],swing=\"2/3\"",
    ] {
        assert!(
            parse_profile(
                &format!("name=\"swing\"\ntitle=\"Swing\"\n[body]\nrhythm={{{rhythm}}}"),
                None
            )
            .is_err()
        );
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn history_gates_fit_swung_intervals() {
    let profile = include_str!("../../../conformance/history-wind.toml").replace(
        "step = \"1/4 beat\"",
        "step = \"1/4 beat\", swing = \"2/3\"",
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
    let times: Vec<_> = player
        .take_events()
        .notes
        .into_iter()
        .map(|e| e.at.to_string())
        .collect();
    assert_eq!(times, ["1/3", "7/15", "1/2", "23/30", "5/6", "29/30", "1"]);
}
