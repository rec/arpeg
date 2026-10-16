use arpeg_core::{Beat, clock::ClockMode, ports::NotePort};
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn conditional_steps_match_python_at_irregular_polling_intervals() {
    let fixture: toml::Value =
        toml::from_str(include_str!("../../../conformance/conditional-steps.toml")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let mut profile: toml::Value =
            toml::from_str(include_str!("../../../conformance/up.toml")).unwrap();
        let body = profile["body"].as_table_mut().unwrap();
        body.insert("rhythm".into(), case["rhythm"].clone());
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
fn invalid_conditions_are_rejected_before_playback() {
    for condition in [
        "passes=[],out_of=4",
        "passes=[0],out_of=4",
        "passes=[5],out_of=4",
        "passes=[1,1],out_of=4",
        "passes=[1],out_of=0",
        "passes=[true],out_of=4",
        "passes=[1],out_of=1.5",
        "passes=[1],out_of=4,unknown=1",
        "passes=[1]",
        "out_of=4",
    ] {
        let profile = format!(
            "name=\"conditional\"\ntitle=\"Conditional\"\n[body]\nrhythm={{kind=\"pattern\",steps=[{{kind=\"hit\",duration=\"1/4 beat\",condition={{{condition}}}}}]}}"
        );
        assert!(parse_profile(&profile, None).is_err(), "{condition}");
    }
}
