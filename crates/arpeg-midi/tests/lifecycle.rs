use arpeg_core::{
    clock::ClockMode,
    ports::{NoteEvent, NotePort},
};
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn lifecycle_matches_shared_delivered_midi_with_irregular_polling() {
    let traces: toml::Value =
        toml::from_str(include_str!("../../../conformance/lifecycle.toml")).unwrap();
    for case in traces["cases"].as_array().unwrap() {
        let profile = match case["profile"].as_str().unwrap() {
            "up" => include_str!("../../../conformance/up.toml"),
            "live-wind" => include_str!("../../../conformance/live-wind.toml"),
            "phrase-wind" => include_str!("../../../conformance/phrase-wind.toml"),
            "lifecycle-zero" => include_str!("../../../conformance/lifecycle-zero.toml"),
            "lifecycle-repeats" => include_str!("../../../conformance/lifecycle-repeats.toml"),
            _ => panic!("unknown profile"),
        };
        for poll_us in [None, Some(1000)] {
            let mut player = MidiPlayer::new(
                parse_profile(profile, None).unwrap(),
                ClockMode::Internal,
                120,
                500_000,
            )
            .unwrap();
            let mut previous = 0;
            for action in case["actions"].as_array().unwrap() {
                let at = action["at"].as_integer().unwrap();
                let mut output = Vec::new();
                let mut notes = Vec::new();
                if let Some(poll_us) = poll_us {
                    let mut tick = previous + poll_us;
                    while tick < at {
                        output.extend(player.advance(tick).unwrap());
                        let batch = player.take_events();
                        assert!(!batch.exhausted);
                        notes.extend(batch.notes);
                        tick += poll_us;
                    }
                }
                output.extend(if let Some(data) = action.get("data") {
                    let data: Vec<u8> = data
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|n| n.as_integer().unwrap() as u8)
                        .collect();
                    player.accept(at, &data, InputSource::Both).unwrap()
                } else if let Some(command) = action.get("capture") {
                    player.capture(at, command.as_str().unwrap()).unwrap()
                } else if action.get("stop").is_some() {
                    player.stop(at).unwrap()
                } else if action.get("clear").is_some() {
                    player.clear(at).unwrap()
                } else {
                    player.advance(at).unwrap()
                });
                let batch = player.take_events();
                assert!(!batch.exhausted);
                notes.extend(batch.notes);
                let expected: Vec<Vec<u8>> = action["output"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| {
                        n.as_array()
                            .unwrap()
                            .iter()
                            .map(|v| v.as_integer().unwrap() as u8)
                            .collect()
                    })
                    .collect();
                assert_eq!(output, expected, "{action}");
                let expected: Vec<NoteEvent> = action["notes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| {
                        let n = n.as_array().unwrap();
                        NoteEvent {
                            at: n[0].as_str().unwrap().parse().unwrap(),
                            port: match n[1].as_str().unwrap() {
                                "note_start" => NotePort::NoteStart,
                                "note_end" => NotePort::NoteEnd,
                                _ => panic!("unknown note port"),
                            },
                            occurrence: n[2].as_integer().unwrap() as u64,
                            source: n[3].as_str().unwrap().into(),
                            key: n[4].as_integer().unwrap() as u8,
                            velocity: n[5].as_integer().unwrap() as u8,
                        }
                    })
                    .collect();
                assert_eq!(notes, expected, "{action}");
                previous = at;
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn full_note_buffer_skips_repeats_but_preserves_ends_and_resumes_future_attacks() {
    let mut player = MidiPlayer::new(
        parse_profile(
            include_str!("../../../conformance/lifecycle-repeats.toml"),
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
    let output = player.advance(125_000_000).unwrap();
    let batch = player.take_events();
    assert!(batch.exhausted);
    assert_eq!(batch.notes.len(), 4096);
    assert_eq!(output.len(), 4096);
    for (index, pair) in batch.notes.chunks_exact(2).enumerate() {
        assert_eq!(pair[0].occurrence, index as u64);
        assert_eq!(pair[0].occurrence, pair[1].occurrence);
        assert_eq!(pair[0].port, NotePort::NoteStart);
        assert_eq!(pair[1].port, NotePort::NoteEnd);
    }
    assert_eq!(
        player.advance(125_050_000).unwrap(),
        [vec![144, 60, 100], vec![128, 60, 0]]
    );
    assert_eq!(
        player
            .take_events()
            .notes
            .iter()
            .map(|n| n.occurrence)
            .collect::<Vec<_>>(),
        [2048, 2048]
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn owned_cleanup_uses_its_reserved_note_slot() {
    let mut player = MidiPlayer::new(
        parse_profile(include_str!("../../../conformance/live-wind.toml"), None).unwrap(),
        ClockMode::Internal,
        120,
        500_000,
    )
    .unwrap();
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    player.advance(255_875_000).unwrap();
    assert_eq!(player.stop(255_876_000).unwrap(), [vec![128, 60, 0]]);
    let batch = player.take_events();
    assert!(!batch.exhausted);
    assert_eq!(batch.notes.len(), 4096);
    assert_eq!(batch.notes.last().unwrap().port, NotePort::NoteEnd);
    assert_eq!(batch.notes.last().unwrap().occurrence, 2047);
}
