use arpeg_core::clock::ClockMode;
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn tuning_wire_traces_match_python() {
    let cases: toml::Value =
        toml::from_str(include_str!("../../../conformance/tuning.toml")).unwrap();
    for case in cases["cases"].as_array().unwrap() {
        let mut profile: toml::Value =
            toml::from_str(include_str!("../../../conformance/up.toml")).unwrap();
        let body = profile["body"].as_table_mut().unwrap();
        for (key, value) in case["body"].as_table().unwrap() {
            body.insert(key.clone(), value.clone());
        }
        let mut player = MidiPlayer::new(
            parse_profile(&toml::to_string(&profile).unwrap(), None).unwrap(),
            ClockMode::Internal,
            120,
            500000,
        )
        .unwrap();
        for input in case["input"].as_array().unwrap() {
            let data: Vec<_> = input
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_integer().unwrap() as u8)
                .collect();
            assert!(
                player
                    .accept(0, &data, InputSource::Both)
                    .unwrap()
                    .is_empty()
            );
        }
        let expected: Vec<Vec<u8>> = case["output"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                r.as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_integer().unwrap() as u8)
                    .collect()
            })
            .collect();
        assert_eq!(player.advance(0).unwrap(), expected, "{}", case["name"]);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn finite_tunings_and_destination_ranges_are_explicit() {
    let base =
        "name=\"tuning\"\ntitle=\"Tuning\"\n[body]\nrhythm={kind=\"grid\",step=\"1 beat\"}\n";
    for fields in [
        "tuning={source={kind=\"computed\"}}",
        "bend_range=2",
        "bend_range=0\ntuning={source={kind=\"computed\"}}",
        "bend_range=2\nvoicing=[1]\ntuning={source={kind=\"frequencies\",values=[\"440\"]}}",
        "bend_range=2\noctaves={offsets=[1]}\ntuning={source={kind=\"ratios\",values=[\"1\"]}}",
    ] {
        assert!(
            parse_profile(&format!("{base}{fields}"), None).is_err(),
            "{fields}"
        );
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn recorded_bends_preserve_tuning_and_release_velocity() {
    let profile = include_str!("../../../conformance/history-wind.toml").replace(
        "[body]",
        "[body]\nbend_range=2\ntuning={source={kind=\"computed\",notes_per_octave=19}}",
    );
    let mut player = MidiPlayer::new(
        parse_profile(&profile, None).unwrap(),
        ClockMode::Internal,
        120,
        500000,
    )
    .unwrap();
    for (at, data) in [
        (0, vec![224, 0, 64]),
        (0, vec![144, 70, 100]),
        (8000, vec![224, 0, 96]),
        (50000, vec![128, 70, 20]),
    ] {
        player.accept(at, &data, InputSource::Both).unwrap();
    }
    assert_eq!(
        player.advance(225000).unwrap(),
        [
            vec![224, 27, 52],
            vec![224, 27, 52],
            vec![144, 70, 100],
            vec![224, 27, 84],
            vec![128, 70, 20]
        ]
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn tuning_expression_and_coordinate_limits_report_errors() {
    for frequency in ["1e-2147483648", "1/0", "(-2)^(1/2)", "2^1025"] {
        let profile = format!(
            "name=\"tuning\"\ntitle=\"Tuning\"\n[body]\nrhythm={{kind=\"grid\",step=\"1 beat\"}}\nbend_range=2\ntuning={{source={{kind=\"frequencies\",values=[\"{frequency}\"]}}}}"
        );
        assert!(parse_profile(&profile, None).is_err());
    }
    let profile = "name=\"tuning\"\ntitle=\"Tuning\"\n[body]\nrhythm={kind=\"grid\",step=\"1 beat\"}\nbend_range=2\ntuning={source={kind=\"computed\",notes_per_octave=9223372036854775807}}\ninversion=-9223372036854775808\nvoicing=[-9223372036854775808]\noctaves={offsets=[-9223372036854775808]}";
    let mut player = MidiPlayer::new(
        parse_profile(profile, None).unwrap(),
        ClockMode::Internal,
        120,
        500000,
    )
    .unwrap();
    player
        .accept(0, &[144, 69, 100], InputSource::Both)
        .unwrap();
    assert!(player.advance(0).is_err());
}
