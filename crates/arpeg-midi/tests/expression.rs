use arpeg_core::{clock::ClockMode, ports::InputPort};
use arpeg_midi::{
    expression::{Lane, motion_message},
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn normalized_samples_round_to_shared_destination_values() {
    let data: toml::Value =
        toml::from_str(include_str!("../../../conformance/expression-values.toml")).unwrap();
    for case in data["cases"].as_array().unwrap() {
        let lane = match case["port"].as_str().unwrap() {
            "breath" => Lane::Breath,
            "bend" => Lane::Bend,
            "pressure" => Lane::Pressure,
            _ => panic!("unknown lane"),
        };
        let expected: Vec<u8> = case["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_integer().unwrap() as u8)
            .collect();
        assert_eq!(
            motion_message(lane, case["value"].as_str().unwrap().parse().unwrap()).unwrap(),
            expected
        );
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn expression_errors_leave_pending_music_and_time_unchanged() {
    for (text, port, value) in [
        (
            include_str!("../../../conformance/up.toml"),
            InputPort::Breath,
            "1/2",
        ),
        (
            include_str!("../../../conformance/motion-expression.toml"),
            InputPort::Bend,
            "2",
        ),
    ] {
        let mut player = MidiPlayer::new(
            parse_profile(text, None).unwrap(),
            ClockMode::Internal,
            120,
            500_000,
        )
        .unwrap();
        player
            .accept(0, &[144, 60, 100], InputSource::Both)
            .unwrap();
        assert!(
            player
                .control(50_000, port, value.parse().unwrap())
                .is_err()
        );
        assert_eq!(player.advance(0).unwrap(), [vec![144, 60, 100]]);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn unknown_motion_state_and_repeated_same_time_samples_are_preserved() {
    let mut player = MidiPlayer::new(
        parse_profile(
            include_str!("../../../conformance/motion-expression.toml"),
            None,
        )
        .unwrap(),
        ClockMode::Internal,
        120,
        500_000,
    )
    .unwrap();
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    assert_eq!(player.advance(0).unwrap(), [vec![144, 60, 100]]);
    for _ in 0..2 {
        assert_eq!(
            player
                .control(0, InputPort::Breath, "1/3".parse().unwrap())
                .unwrap(),
            [vec![176, 2, 42]]
        );
    }
    assert_eq!(
        player.advance(125_000).unwrap(),
        [vec![128, 60, 0], vec![176, 2, 42], vec![144, 60, 100]]
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn recorded_override_keeps_its_gesture_with_global_current_source() {
    let text = include_str!("../../../conformance/motion-phrase.toml")
        .replace("source = \"recorded\"", "source = \"current\"")
        .replace("pressure = \"current\"", "bend = \"recorded\"");
    let mut player = MidiPlayer::new(
        parse_profile(&text, None).unwrap(),
        ClockMode::Internal,
        120,
        500_000,
    )
    .unwrap();
    player.accept(0, &[224, 0, 64], InputSource::Both).unwrap();
    player.capture(0, "record").unwrap();
    player
        .control(0, InputPort::Breath, "1/2".parse().unwrap())
        .unwrap();
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    player
        .accept(10_000, &[224, 0, 96], InputSource::Both)
        .unwrap();
    player
        .accept(20_000, &[128, 60, 0], InputSource::Both)
        .unwrap();
    player.capture(20_000, "commit").unwrap();
    assert_eq!(
        player.advance(125_000).unwrap(),
        [vec![176, 2, 64], vec![224, 0, 64], vec![144, 60, 100]]
    );
    assert_eq!(
        player
            .control(125_000, InputPort::Breath, 1.into())
            .unwrap(),
        [vec![176, 2, 127]]
    );
    assert_eq!(player.advance(175_000).unwrap(), [vec![224, 0, 96]]);
}
