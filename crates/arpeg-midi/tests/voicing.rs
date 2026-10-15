use arpeg_core::clock::ClockMode;
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn voicing_traces_match_python() {
    let traces: toml::Value =
        toml::from_str(include_str!("../../../conformance/voicing.toml")).unwrap();
    for case in traces["cases"].as_array().unwrap() {
        let mut profile: toml::Value =
            toml::from_str(include_str!("../../../conformance/up.toml")).unwrap();
        let body = profile["body"].as_table_mut().unwrap();
        for name in [
            "inversion",
            "selection",
            "octaves",
            "scale",
            "voicing",
            "selection_offset",
        ] {
            if let Some(value) = case.get(name) {
                body.insert(name.into(), value.clone());
            }
        }
        if let Some(limits) = case.get("limits") {
            body.insert("register_limits".into(), limits.clone());
        }
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
fn voicing_profiles_reject_invalid_lists_and_banks_without_chords() {
    for count in ["true", "[true]", "[]", "[1.5]", "[9223372036854775808]"] {
        let profile = format!(
            "name=\"inversion\"\ntitle=\"Inversion\"\n[body]\nvoicing={count}\nrhythm={{kind=\"grid\",step=\"1/4 beat\"}}"
        );
        assert!(parse_profile(&profile, None).is_err());
    }
    for kind in ["history", "phrase"] {
        let profile = include_str!("../../../conformance/history-wind.toml")
            .replace("[body]", "[body]\nvoicing=[0]")
            .replace(
                "kind = \"history\", notes = 8",
                &format!("kind = \"{kind}\""),
            );
        assert!(
            parse_profile(&profile, None)
                .err()
                .unwrap()
                .contains("voicing requires")
        );
    }
}
