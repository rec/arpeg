use arpeg_core::{Beat, clock::ClockMode};
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn transport_matches_shared_exact_wire_traces() {
    for text in [
        include_str!("../../../conformance/transport.toml"),
        include_str!("../../../conformance/performance.toml"),
        include_str!("../../../conformance/phrase.toml"),
    ] {
        let traces: toml::Value = toml::from_str(text).unwrap();
        for case in traces["cases"].as_array().unwrap() {
            let name = case["name"].as_str().unwrap();
            let text = match case["profile"].as_str().unwrap() {
                "up" => include_str!("../../../conformance/up.toml"),
                "history-wind" => include_str!("../../../conformance/history-wind.toml"),
                "live-wind" => include_str!("../../../conformance/live-wind.toml"),
                "history-live-wind" => include_str!("../../../conformance/history-live-wind.toml"),
                "phrase-wind" => include_str!("../../../conformance/phrase-wind.toml"),
                _ => panic!("unknown profile"),
            };
            let mode = if case["mode"].as_str() == Some("internal") {
                ClockMode::Internal
            } else {
                ClockMode::External
            };
            for poll_us in [None, Some(1000)] {
                let mut player = MidiPlayer::new(
                    parse_profile(text, None).unwrap(),
                    mode,
                    120,
                    case.get("timeout_us")
                        .and_then(toml::Value::as_integer)
                        .unwrap_or(500_000),
                )
                .unwrap();
                let mut previous_at = 0;
                for action in case["actions"].as_array().unwrap() {
                    let at = action["at"].as_integer().unwrap();
                    let mut output = Vec::new();
                    if let Some(poll_us) = poll_us {
                        let mut tick = previous_at + poll_us;
                        while tick < at {
                            output.extend(player.advance(tick).unwrap());
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
                        let source = match action.get("source").and_then(toml::Value::as_str) {
                            Some("notes") => InputSource::Notes,
                            Some("clock") => InputSource::Clock,
                            _ => InputSource::Both,
                        };
                        player.accept(at, &data, source).unwrap()
                    } else if let Some(bpm) = action.get("tempo") {
                        player
                            .set_tempo(at, bpm.as_integer().unwrap().try_into().unwrap())
                            .unwrap()
                    } else if let Some(command) = action.get("capture") {
                        player.capture(at, command.as_str().unwrap()).unwrap()
                    } else if action.get("clear").is_some() {
                        player.clear(at).unwrap()
                    } else {
                        player.advance(at).unwrap()
                    });
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
                    let beat: Beat = action["beat"].as_str().unwrap().parse().unwrap();
                    assert_eq!(output, expected, "{name}: {action}");
                    assert_eq!(player.clock.beat, beat, "{name}: {action}");
                    if let Some(state) = action.get("state") {
                        let state = state.as_array().unwrap();
                        assert_eq!(
                            player.capture_state(),
                            Some((
                                state[0].as_bool().unwrap(),
                                state[1].as_integer().unwrap() as usize,
                                state[2].as_integer().unwrap() as usize
                            )),
                            "{name}: {action}"
                        );
                    }
                    assert_eq!(
                        player.clock.active(),
                        action["active"].as_bool().unwrap(),
                        "{name}: {action}"
                    );
                    previous_at = at;
                }
            }
        }
    }
}
