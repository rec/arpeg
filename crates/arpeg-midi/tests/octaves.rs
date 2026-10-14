use arpeg_core::clock::ClockMode;
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn expanded_octave_traces_match_python() {
    let traces: toml::Value =
        toml::from_str(include_str!("../../../conformance/octaves.toml")).unwrap();
    for case in traces["cases"].as_array().unwrap() {
        let mut player = MidiPlayer::new(
            parse_profile(case["profile"].as_str().unwrap(), None).unwrap(),
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
                        0x90,
                        note[0].as_integer().unwrap() as u8,
                        note[1].as_integer().unwrap() as u8,
                    ],
                    InputSource::Both,
                )
                .unwrap();
        }
        let mut keys = Vec::new();
        for i in 0..case["keys"].as_array().unwrap().len() {
            for message in player.advance(i as i64 * 125_000).unwrap() {
                if message[0] == 0x90 && message[2] > 0 {
                    keys.push(i64::from(message[1]));
                }
            }
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
fn history_replays_source_expression_in_each_octave() {
    let text = include_str!("../../../conformance/history-wind.toml")
        .replace("[body]", "[body]\noctaves = { offsets = [-1, 0, 1] }");
    let mut player = MidiPlayer::new(
        parse_profile(&text, None).unwrap(),
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
    let output = player.advance(475000).unwrap();
    for (i, key) in [48, 60, 72].iter().enumerate() {
        assert_eq!(
            output[i * 4..(i + 1) * 4],
            [
                vec![176, 2, 0],
                vec![144, *key, 100],
                vec![176, 2, 80],
                vec![128, *key, 20]
            ]
        );
    }
}
