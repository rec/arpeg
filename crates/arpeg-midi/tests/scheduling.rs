use arpeg_core::{
    Beat,
    clock::ClockMode,
    ports::{InputPort, NotePort, OutputPort},
};
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn expression_precedes_attacks_in_the_shared_wire_trace() {
    let trace: toml::Value = toml::from_str(include_str!(
        "../../../conformance/pre-note-expression.toml"
    ))
    .unwrap();
    for case in trace["cases"].as_array().unwrap() {
        let mut player = player(case["profile"].as_str().unwrap(), ClockMode::Internal);
        for action in case["actions"].as_array().unwrap() {
            let tick = action["at_us"].as_integer().unwrap();
            let mut output = vec![];
            if let Some(data) = action.get("data") {
                for data in data.as_array().unwrap() {
                    let bytes: Vec<u8> = data
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_integer().unwrap().try_into().unwrap())
                        .collect();
                    output.extend(player.accept(tick, &bytes, InputSource::Both).unwrap());
                }
            }
            if let Some(capture) = action.get("capture") {
                output.extend(player.capture(tick, capture.as_str().unwrap()).unwrap());
            }
            if action.get("stop").and_then(toml::Value::as_bool) == Some(true) {
                output.extend(player.stop(tick).unwrap());
            }
            if action.get("schedule").and_then(toml::Value::as_bool) == Some(true) {
                player
                    .schedule(
                        tick,
                        action
                            .get("tempo")
                            .map(|v| v.as_integer().unwrap().try_into().unwrap()),
                    )
                    .unwrap();
                assert!(!player.take_events().exhausted);
                let controls: Vec<_> = action
                    .get("controls")
                    .into_iter()
                    .flat_map(|v| v.as_array().unwrap())
                    .map(|v| {
                        let values = v.as_array().unwrap();
                        let port = match values[0].as_str().unwrap() {
                            "breath" => InputPort::Breath,
                            "bend" => InputPort::Bend,
                            "pressure" => InputPort::Pressure,
                            _ => panic!("unknown expression lane"),
                        };
                        (port, values[1].as_str().unwrap().parse::<Beat>().unwrap())
                    })
                    .collect();
                output.extend(player.realize(&controls).unwrap());
            }
            let expected: Vec<Vec<u8>> = action["output"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    v.as_array()
                        .unwrap()
                        .iter()
                        .map(|n| n.as_integer().unwrap().try_into().unwrap())
                        .collect()
                })
                .collect();
            assert_eq!(output, expected, "{}: {action}", case["name"]);
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn hits_precede_midi_but_note_lifecycle_follows_realization_once() {
    let mut player = player("envelfo-expression", ClockMode::Internal);
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    player.schedule(0, None).unwrap();
    let batch = player.take_events();
    assert_eq!(batch.events.last().unwrap().port, OutputPort::Hit);
    assert!(batch.notes.is_empty());
    assert_eq!(
        player.realize(&[(InputPort::Breath, 1.into())]).unwrap(),
        [vec![176, 2, 127], vec![144, 60, 100]]
    );
    let batch = player.take_events();
    assert!(batch.events.is_empty());
    assert_eq!(batch.notes.len(), 1);
    assert_eq!(batch.notes[0].port, NotePort::NoteStart);
    assert!(player.realize(&[]).is_err());
    assert!(player.advance(0).unwrap().is_empty());
    assert!(player.take_events().notes.is_empty());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn pending_phase_rejects_mutation_and_invalid_controls_without_losing_music() {
    let mut player = player("envelfo-phrase", ClockMode::Internal);
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    player.schedule(0, None).unwrap();
    assert!(player.accept(0, &[144, 64, 90], InputSource::Both).is_err());
    assert!(player.schedule(0, None).is_err());
    assert!(player.advance(0).is_err());
    assert!(player.set_tempo(0, 60).is_err());
    assert!(player.clear(0).is_err());
    assert!(player.capture(0, "record").is_err());
    assert!(player.control(0, InputPort::Breath, 1.into()).is_err());
    assert!(player.realize(&[(InputPort::Gate, 1.into())]).is_err());
    assert!(
        player
            .realize(&[(InputPort::Breath, 1.into()), (InputPort::Bend, 0.into())])
            .is_err()
    );
    assert!(player.realize(&[(InputPort::Breath, 2.into())]).is_err());
    assert_eq!(player.clock.beat, 0.into());
    assert!(
        player
            .realize(&[(InputPort::Breath, 1.into())])
            .unwrap()
            .is_empty()
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn expression_cannot_span_multiple_attack_boundaries() {
    let mut player = player("envelfo-expression", ClockMode::Internal);
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    player.schedule(125_000, None).unwrap();
    assert!(player.realize(&[(InputPort::Breath, 1.into())]).is_err());
    assert_eq!(
        player.realize(&[]).unwrap(),
        [vec![144, 60, 100], vec![128, 60, 0], vec![144, 60, 100]]
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn stop_cancels_unsent_attacks_and_releases_only_the_actual_note() {
    for (profile, sounding) in [
        ("envelfo-expression", false),
        ("envelfo-expression", true),
        ("envelfo-phrase", false),
        ("envelfo-phrase", true),
    ] {
        let mut player = player(profile, ClockMode::Internal);
        let (first, next) = if profile == "envelfo-phrase" {
            player.capture(0, "record").unwrap();
            for (tick, data) in [
                (0, [144, 60, 100]),
                (10_000, [128, 60, 20]),
                (10_000, [144, 64, 90]),
                (20_000, [128, 64, 20]),
            ] {
                player.accept(tick, &data, InputSource::Both).unwrap();
            }
            player.capture(20_000, "commit").unwrap();
            (125_000, 250_000)
        } else {
            player
                .accept(0, &[144, 60, 100], InputSource::Both)
                .unwrap();
            player.accept(0, &[144, 64, 90], InputSource::Both).unwrap();
            (0, 125_000)
        };
        if sounding {
            player.advance(first).unwrap();
            player.take_events();
        }
        let tick = if sounding { next } else { first };
        player.schedule(tick, None).unwrap();
        assert_eq!(
            player.stop(tick).unwrap(),
            if sounding {
                vec![vec![128, 60, 0]]
            } else {
                vec![]
            }
        );
        let notes = player.take_events().notes;
        assert_eq!(notes.len(), usize::from(sounding));
        if sounding {
            assert_eq!(notes[0].port, NotePort::NoteEnd);
            assert_eq!(notes[0].key, 60);
        }
        assert!(player.advance(500_000).unwrap().is_empty());
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn fractional_attack_boundaries_survive_host_clock_rounding() {
    let text = include_str!("../../../conformance/envelfo-expression.toml")
        .replace("1/4 beat", "1/7 beat");
    let mut player = MidiPlayer::new(
        parse_profile(&text, None).unwrap(),
        ClockMode::Internal,
        120,
        500_000,
    )
    .unwrap();
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    for (i, tick) in [0, 71_429, 142_858].into_iter().enumerate() {
        player.schedule(tick, None).unwrap();
        let batch = player.take_events();
        assert_eq!(
            batch
                .events
                .iter()
                .filter(|e| e.port == OutputPort::Hit)
                .map(|e| e.at)
                .collect::<Vec<_>>(),
            [Beat::new(i as i64, 7)]
        );
        let output = player
            .realize(&[(InputPort::Breath, Beat::new(i as i64 + 1, 4))])
            .unwrap();
        let mut expected = if i == 0 {
            vec![]
        } else {
            vec![vec![128, 60, 0]]
        };
        expected.extend([vec![176, 2, [32, 64, 95][i]], vec![144, 60, 100]]);
        assert_eq!(output, expected);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn external_clock_timeout_releases_before_discarding_new_expression() {
    let mut player = player("envelfo-expression", ClockMode::External);
    for data in [vec![250], vec![248], vec![144, 60, 100]] {
        player.accept(0, &data, InputSource::Both).unwrap();
    }
    player.schedule(0, None).unwrap();
    assert_eq!(
        player.realize(&[(InputPort::Breath, 1.into())]).unwrap(),
        [vec![176, 2, 127], vec![144, 60, 100]]
    );
    player.schedule(500_000, None).unwrap();
    assert_eq!(
        player.realize(&[(InputPort::Breath, 0.into())]).unwrap(),
        [vec![128, 60, 0]]
    );
}

fn player(profile: &str, mode: ClockMode) -> MidiPlayer {
    let text = match profile {
        "envelfo-expression" => include_str!("../../../conformance/envelfo-expression.toml"),
        "envelfo-phrase" => include_str!("../../../conformance/envelfo-phrase.toml"),
        _ => panic!("unknown profile"),
    };
    MidiPlayer::new(parse_profile(text, None).unwrap(), mode, 120, 500_000).unwrap()
}
