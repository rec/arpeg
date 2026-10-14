use arpeg_core::clock::ClockMode;
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn register_traces_match_python() {
    let traces: toml::Value =
        toml::from_str(include_str!("../../../conformance/register.toml")).unwrap();
    for case in traces["cases"].as_array().unwrap() {
        let mut profile: toml::Value =
            toml::from_str(include_str!("../../../conformance/up.toml")).unwrap();
        let body = profile["body"].as_table_mut().unwrap();
        body.insert("register_limits".into(), case["limits"].clone());
        body.insert(
            "octaves".into(),
            toml::Value::Table(toml::Table::from_iter([(
                "offsets".into(),
                case.get("octaves")
                    .cloned()
                    .unwrap_or(toml::Value::Array(vec![toml::Value::Integer(0)])),
            )])),
        );
        body.insert(
            "transposition".into(),
            toml::Value::Table(toml::Table::from_iter([
                (
                    "semitones".into(),
                    case.get("semitones")
                        .cloned()
                        .unwrap_or(toml::Value::Integer(0)),
                ),
                (
                    "boundary".into(),
                    case.get("midi_boundary")
                        .cloned()
                        .unwrap_or(toml::Value::String("drop".into())),
                ),
            ])),
        );
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
        let mut keys = Vec::new();
        for i in 0..case["keys"].as_array().unwrap().len() {
            let output = player.advance(i as i64 * 125_000).unwrap();
            keys.push(
                output
                    .iter()
                    .find(|m| m[0] == 144 && m[2] > 0)
                    .map_or(-1, |m| i64::from(m[1])),
            );
        }
        let expected: Vec<_> = case["keys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_integer().unwrap())
            .collect();
        assert_eq!(keys, expected, "{}", case["name"]);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn folded_history_preserves_expression_and_release_velocity() {
    let profile = include_str!("../../../conformance/history-wind.toml").replace(
        "[body]",
        "[body]\nregister_limits={low=72,high=84,boundary=\"fold\"}",
    );
    let mut player = MidiPlayer::new(
        parse_profile(&profile, None).unwrap(),
        ClockMode::Internal,
        120,
        500_000,
    )
    .unwrap();
    for (at, data) in [
        (0, vec![176, 2, 0]),
        (0, vec![144, 60, 100]),
        (8000, vec![176, 2, 80]),
        (50000, vec![128, 60, 20]),
    ] {
        player.accept(at, &data, InputSource::Both).unwrap();
    }
    assert_eq!(
        player.advance(225000).unwrap(),
        [
            vec![176, 2, 0],
            vec![144, 72, 100],
            vec![176, 2, 80],
            vec![128, 72, 20]
        ]
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn register_errors_preserve_owned_cleanup_and_invalid_bounds_are_rejected() {
    let profile = "name=\"register\"\ntitle=\"Register\"\n[body]\nrhythm={kind=\"grid\",step=\"1/4 beat\"}\ngate=2\nregister_limits={low=60,high=72,boundary=\"error\"}";
    let mut player = MidiPlayer::new(
        parse_profile(profile, None).unwrap(),
        ClockMode::Internal,
        120,
        500_000,
    )
    .unwrap();
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    player.accept(0, &[144, 73, 90], InputSource::Both).unwrap();
    assert_eq!(player.advance(0).unwrap(), [vec![144, 60, 100]]);
    assert!(
        player
            .advance(125000)
            .unwrap_err()
            .contains("register limits")
    );
    assert_eq!(player.stop(125000).unwrap(), [vec![128, 60, 0]]);
    for bounds in [
        "low=73,high=72",
        "low=-1",
        "high=128",
        "low=true",
        "high=60.5",
        "boundary=\"clamp\"",
    ] {
        let invalid = format!(
            "name=\"register\"\ntitle=\"Register\"\n[body]\nrhythm={{kind=\"grid\",step=\"1/4 beat\"}}\nregister_limits={{{bounds}}}"
        );
        assert!(parse_profile(&invalid, None).is_err(), "{bounds}");
    }
}
