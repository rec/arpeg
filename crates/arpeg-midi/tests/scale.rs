use arpeg_core::clock::ClockMode;
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn scale_traces_match_python() {
    let traces: toml::Value =
        toml::from_str(include_str!("../../../conformance/scale.toml")).unwrap();
    for case in traces["cases"].as_array().unwrap() {
        let mut profile: toml::Value =
            toml::from_str(include_str!("../../../conformance/up.toml")).unwrap();
        let body = profile["body"].as_table_mut().unwrap();
        body.insert("scale".into(), case["scale"].clone());
        if let Some(limits) = case.get("limits") {
            body.insert("register_limits".into(), limits.clone());
        }
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
fn invalid_scales_are_rejected() {
    for scale in [
        "root=-1,intervals=[0]",
        "root=12,intervals=[0]",
        "root=true,intervals=[0]",
        "root=0,intervals=[]",
        "root=0,intervals=[1,3]",
        "root=0,intervals=[0,0]",
        "root=0,intervals=[0,7,3]",
        "root=0,intervals=[0,12]",
        "root=0,intervals=[0,2.5]",
        "definition={note_names=\"A\",begin=\"A\",end=\"A\",root=\"A\",notes=\"A\",intervals=[12],offset=0},degrees=true",
        "definition={note_names=\"A\",begin=\"A\",end=\"A\",root=\"A\",notes=\"A\",intervals=[12],offset=0},mapping=\"preserve\"",
    ] {
        let profile = format!(
            "name=\"scale\"\ntitle=\"Scale\"\n[body]\nrhythm={{kind=\"grid\",step=\"1/4 beat\"}}\nscale={{{scale}}}"
        );
        assert!(parse_profile(&profile, None).is_err(), "{scale}");
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn scale_history_preserves_expression_and_release_velocity() {
    let profile = include_str!("../../../conformance/history-wind.toml").replace(
        "[body]",
        "[body]\nscale={definition={notes=\"CDEFGAB\"},degrees=1}",
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
            vec![144, 62, 100],
            vec![176, 2, 80],
            vec![128, 62, 20]
        ]
    );
}
