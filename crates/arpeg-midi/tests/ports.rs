use arpeg_core::{
    clock::ClockMode,
    ports::{InputPort, OutputPort, PortEvent},
};
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

fn player(text: &str) -> MidiPlayer {
    MidiPlayer::new(
        parse_profile(text, None).unwrap(),
        ClockMode::Internal,
        120,
        500_000,
    )
    .unwrap()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn pitch_range_errors_still_release_the_last_delivered_note() {
    for (text, captured) in [
        (include_str!("../../../conformance/up.toml"), false),
        (include_str!("../../../conformance/phrase-wind.toml"), true),
    ] {
        let mut player = player(&format!(
            "{text}\n[body.transposition]\nboundary = \"error\"\n"
        ));
        if captured {
            player.capture(0, "record").unwrap();
        }
        player
            .accept(0, &[144, 120, 100], InputSource::Both)
            .unwrap();
        if captured {
            player.capture(0, "commit").unwrap();
        }
        assert_eq!(player.advance(0).unwrap(), [vec![144, 120, 100]]);
        player
            .control(50_000, InputPort::Transposition, 12.into())
            .unwrap();
        assert_eq!(
            player.advance(250_000),
            Err("transposed pitch is outside MIDI range 0–127")
        );
        assert_eq!(player.stop(250_000).unwrap(), [vec![128, 120, 0]]);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn fractional_transposition_preserves_time_and_pending_music() {
    let mut player = player(include_str!("../../../conformance/up.toml"));
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    assert!(
        player
            .control(50_000, InputPort::Transposition, "1/2".parse().unwrap())
            .is_err()
    );
    assert_eq!(player.advance(0).unwrap(), [vec![144, 60, 100]]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn motion_ports_match_shared_exact_traces() {
    let traces: toml::Value =
        toml::from_str(include_str!("../../../conformance/ports.toml")).unwrap();
    for case in traces["cases"].as_array().unwrap() {
        let text = match case["profile"].as_str().unwrap() {
            "up" => include_str!("../../../conformance/up.toml"),
            "custom-steps" => include_str!("../../../conformance/custom-steps.toml"),
            "phrase-wind" => include_str!("../../../conformance/phrase-wind.toml"),
            "motion-ports" => include_str!("../../../conformance/motion-ports.toml"),
            "transpose-fold" => include_str!("../../../conformance/transpose-fold.toml"),
            _ => panic!("unknown profile"),
        };
        for poll_us in [None, Some(1000)] {
            let mut player = player(text);
            let mut previous_at = 0;
            for action in case["actions"].as_array().unwrap() {
                let at = action["at"].as_integer().unwrap();
                let mut output = Vec::new();
                let mut events = Vec::new();
                if let Some(poll_us) = poll_us {
                    let mut tick = previous_at + poll_us;
                    while tick < at {
                        output.extend(player.advance(tick).unwrap());
                        let batch = player.take_events();
                        assert!(!batch.exhausted);
                        events.extend(batch.events);
                        tick += poll_us;
                    }
                }
                output.extend(if let Some(data) = action.get("data") {
                    let data: Vec<u8> = data
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_integer().unwrap().try_into().unwrap())
                        .collect();
                    player.accept(at, &data, InputSource::Both).unwrap()
                } else if let Some(command) = action.get("capture") {
                    player.capture(at, command.as_str().unwrap()).unwrap()
                } else if let Some(port) = action.get("port") {
                    let port = match port.as_str().unwrap() {
                        "gate" => InputPort::Gate,
                        "density" => InputPort::Density,
                        "transposition" => InputPort::Transposition,
                        _ => panic!("unknown port"),
                    };
                    player
                        .control(at, port, action["value"].as_str().unwrap().parse().unwrap())
                        .unwrap()
                } else {
                    player.advance(at).unwrap()
                });
                let batch = player.take_events();
                assert!(!batch.exhausted);
                events.extend(batch.events);
                let expected: Vec<Vec<u8>> = action["output"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| {
                        a.as_array()
                            .unwrap()
                            .iter()
                            .map(|v| v.as_integer().unwrap().try_into().unwrap())
                            .collect()
                    })
                    .collect();
                assert_eq!(output, expected, "{action}");
                let expected: Vec<PortEvent> = action["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| {
                        let a = e.as_array().unwrap();
                        PortEvent {
                            at: a[0].as_str().unwrap().parse().unwrap(),
                            port: match a[1].as_str().unwrap() {
                                "step" => OutputPort::Step,
                                "hit" => OutputPort::Hit,
                                "rest" => OutputPort::Rest,
                                _ => panic!("unknown output port"),
                            },
                            index: a[2].as_integer().unwrap(),
                            revision: a[3].as_integer().unwrap() as u64,
                        }
                    })
                    .collect();
                assert_eq!(events, expected, "{action}");
                previous_at = at;
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn invalid_controls_leave_pending_music_unchanged() {
    let mut player = player(include_str!("../../../conformance/up.toml"));
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    player.advance(0).unwrap();
    assert!(
        player
            .control(50_000, InputPort::Density, "1/2".parse().unwrap())
            .is_err()
    );
    assert!(
        player
            .control(50_000, InputPort::Gate, "-1".parse().unwrap())
            .is_err()
    );
    assert!(
        player
            .control(50_000, InputPort::Density, "2".parse().unwrap())
            .is_err()
    );
    assert_eq!(player.clock.beat.to_string(), "0");
    assert_eq!(player.advance(100_000).unwrap(), [vec![128, 60, 0]]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn full_event_buffer_skips_attacks_and_still_releases_owned_notes() {
    let mut player = player(include_str!("../../../conformance/up.toml"));
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    let output = player.advance(256_000_000).unwrap();
    assert_eq!(output.iter().filter(|e| e[0] == 144).count(), 2048);
    assert_eq!(output.iter().filter(|e| e[0] == 128).count(), 2048);
    let batch = player.take_events();
    assert_eq!(batch.events.len(), 4096);
    assert!(batch.exhausted);
    assert_eq!(player.advance(256_125_000).unwrap(), [vec![144, 60, 100]]);
    assert!(!player.take_events().exhausted);
}
