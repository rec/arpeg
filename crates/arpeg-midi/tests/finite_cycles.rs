use arpeg_core::{Beat, clock::ClockMode, ports::NotePort};
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn finite_cycles_match_python_at_irregular_polling_intervals() {
    let fixture: toml::Value =
        toml::from_str(include_str!("../../../conformance/finite-cycles.toml")).unwrap();
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
fn finite_cycle_limits_reject_invalid_values_and_unsupported_rhythms() {
    for rhythm in [
        "kind=\"pattern\",steps=[{kind=\"hit\",duration=\"1/4 beat\"}]",
        "kind=\"euclidean\",steps=3,pulses=2,step=\"1/4 beat\"",
    ] {
        for cycles in ["0", "-1", "true", "1.5", "\"2\""] {
            let profile = format!(
                "name=\"finite\"\ntitle=\"Finite\"\n[body]\nrhythm={{{rhythm},cycles={cycles}}}"
            );
            assert!(parse_profile(&profile, None).is_err(), "{profile}");
        }
    }
    let profile = "name=\"finite\"\ntitle=\"Finite\"\n[body]\nrhythm={kind=\"grid\",step=\"1/4 beat\",cycles=2}";
    assert!(parse_profile(profile, None).is_err());
}
