use arpeg_core::{Beat, clock::ClockMode, ports::NotePort};
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn strums_and_lanes_match_python_traces_at_irregular_polling_intervals() {
    let fixture: toml::Value =
        toml::from_str(include_str!("../../../conformance/strums-lanes.toml")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        for polling in [false, true] {
            let mut player = player(
                &toml::to_string(&case["body"]).unwrap(),
                ClockMode::Internal,
            );
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
            let end =
                (case["through"].as_str().unwrap().parse::<Beat>().unwrap() * 500_000).to_integer();
            let times: Vec<_> = if polling {
                (0..end).step_by(17_000).chain([end]).collect()
            } else {
                vec![end]
            };
            let mut notes = Vec::new();
            let mut wire = Vec::new();
            for at in times {
                wire.extend(player.advance(at).unwrap());
                let batch = player.take_events();
                assert!(!batch.exhausted);
                notes.extend(batch.notes);
            }
            let actual: Vec<_> = notes
                .iter()
                .map(|n| {
                    toml::Value::Array(vec![
                        n.at.to_string().into(),
                        (if n.port == NotePort::NoteStart {
                            "on"
                        } else {
                            "off"
                        })
                        .into(),
                        i64::from(n.channel).into(),
                        i64::from(n.key).into(),
                        i64::from(n.velocity).into(),
                    ])
                })
                .collect();
            assert_eq!(
                toml::Value::Array(actual),
                case["events"],
                "{} polling={polling}",
                case["name"]
            );
            let expected: Vec<Vec<u8>> = case["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| {
                    vec![
                        if e[1].as_str().unwrap() == "on" {
                            143
                        } else {
                            127
                        } + e[2].as_integer().unwrap() as u8,
                        e[3].as_integer().unwrap() as u8,
                        e[4].as_integer().unwrap() as u8,
                    ]
                })
                .collect();
            assert_eq!(wire, expected);
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn strum_snapshot_survives_chord_edits_repeats_and_ties() {
    let mut player = player(
        "strum={spacing='1/8 beat'}\nrhythm={kind='pattern',steps=[{kind='hit',duration='1 beat',repeats=2},{kind='tie',duration='1/2 beat'}]}",
        ClockMode::Internal,
    );
    for key in [60, 64] {
        player
            .accept(0, &[144, key, 90], InputSource::Both)
            .unwrap();
    }
    let mut wire = player.advance(0).unwrap();
    wire.extend(
        player
            .accept(25_000, &[128, 64, 0], InputSource::Both)
            .unwrap(),
    );
    wire.extend(
        player
            .accept(25_000, &[144, 48, 80], InputSource::Both)
            .unwrap(),
    );
    wire.extend(player.advance(700_000).unwrap());
    assert_eq!(
        wire.iter()
            .filter(|m| m[0] == 144)
            .map(|m| m[1])
            .collect::<Vec<_>>(),
        [60, 64, 60, 64]
    );
    assert!(
        player
            .take_events()
            .notes
            .iter()
            .any(|n| n.port == NotePort::NoteEnd && n.at > Beat::from_integer(1))
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn cleanup_cancels_unsent_strums_on_every_lane_channel() {
    for operation in ["stop", "clear", "seek"] {
        let mut player = player(
            "bank={kind='latched'}\nstrum={spacing='1/8 beat'}\nrhythm={kind='grid',step='1 beat'}\n[lanes.other]\nchannel=2\nbank={kind='latched'}\nstrum={spacing='1/8 beat'}\nrhythm={kind='grid',step='1 beat'}",
            ClockMode::Internal,
        );
        for key in [60, 64, 67] {
            player
                .accept(0, &[144, key, 90], InputSource::Both)
                .unwrap();
        }
        player.advance(0).unwrap();
        let output = match operation {
            "stop" => player.stop(25_000).unwrap(),
            "clear" => player.clear(25_000).unwrap(),
            _ => player
                .accept(25_000, &[242, 4, 0], InputSource::Both)
                .unwrap(),
        };
        assert_eq!(output, [vec![128, 60, 0], vec![129, 60, 0]]);
        if operation == "seek" {
            player.stop(25_000).unwrap();
        }
        assert!(player.advance(400_000).unwrap().is_empty());
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn expression_uses_each_sounding_lane_channel_without_releasing_other_notes() {
    let mut player = player(
        "strum={spacing='1/8 beat'}\nrhythm={kind='grid',step='1 beat'}\n[lanes.other]\nchannel=3\nrhythm={kind='grid',step='1 beat'}",
        ClockMode::Internal,
    );
    player.accept(0, &[176, 2, 30], InputSource::Both).unwrap();
    for key in [60, 64] {
        player
            .accept(0, &[144, key, 90], InputSource::Both)
            .unwrap();
    }
    assert_eq!(
        player.advance(100_000).unwrap(),
        [
            vec![176, 2, 30],
            vec![144, 60, 90],
            vec![178, 2, 30],
            vec![146, 60, 90],
            vec![176, 2, 30],
            vec![144, 64, 90]
        ]
    );
    assert_eq!(
        player
            .accept(110_000, &[176, 2, 80], InputSource::Both)
            .unwrap(),
        [vec![176, 2, 80], vec![178, 2, 80]]
    );
    assert_eq!(player.stop(110_000).unwrap().len(), 3);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn incompatible_strum_tuning_is_rejected_with_owned_cleanup_preserved() {
    let mut player = player(
        "strum={spacing='1/8 beat'}\ntuning={source={kind='computed',notes_per_octave=19,octave_ratio='2'},root_note=69,root_frequency='440'}\nbend_range=2",
        ClockMode::Internal,
    );
    for key in [69, 70] {
        player
            .accept(0, &[144, key, 90], InputSource::Both)
            .unwrap();
    }
    player.advance(0).unwrap();
    assert!(
        player
            .advance(100_000)
            .unwrap_err()
            .contains("same pitch bend")
    );
    assert_eq!(player.stop(100_000).unwrap(), [vec![128, 69, 0]]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn polyphonic_event_capacity_reserves_every_owned_release() {
    let mut player = player(
        "strum={spacing='1/128 beat'}\nrhythm={kind='grid',step='1/64 beat'}\ngate=1000",
        ClockMode::Internal,
    );
    for key in [60, 64, 67] {
        player
            .accept(0, &[144, key, 90], InputSource::Both)
            .unwrap();
    }
    player.advance(10_000_000).unwrap();
    let batch = player.take_events();
    assert!(batch.exhausted);
    player.stop(10_000_000).unwrap();
    let mut notes = batch.notes;
    notes.extend(player.take_events().notes);
    let mut starts: Vec<_> = notes
        .iter()
        .filter(|n| n.port == NotePort::NoteStart)
        .map(|n| n.occurrence)
        .collect();
    let mut ends: Vec<_> = notes
        .iter()
        .filter(|n| n.port == NotePort::NoteEnd)
        .map(|n| n.occurrence)
        .collect();
    starts.sort();
    ends.sort();
    assert_eq!(starts, ends);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn external_clock_is_observed_once_for_multiple_lanes() {
    let mut player = player(
        "[lanes.other]\nchannel=2\nrhythm={kind='grid',step='1/3 beat'}",
        ClockMode::External,
    );
    player.accept(0, &[144, 60, 90], InputSource::Both).unwrap();
    for (at, data) in [(0, [250]), (0, [248]), (20_000, [248]), (40_000, [248])] {
        player.accept(at, &data, InputSource::Both).unwrap();
    }
    assert_eq!(player.clock.observations.len(), 4);
    player.advance(40_000).unwrap();
    assert_eq!(player.clock.beat, Beat::new(1, 12));
    assert_eq!(player.stop(40_000).unwrap().len(), 2);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn clear_preserves_held_lanes_and_clears_latched_lanes() {
    let mut player = player(
        "[lanes.latch]\nchannel=2\nbank={kind='latched'}\nrhythm={kind='grid',step='1/4 beat'}",
        ClockMode::Internal,
    );
    player.accept(0, &[144, 60, 90], InputSource::Both).unwrap();
    player.advance(0).unwrap();
    assert_eq!(player.clear(25_000).unwrap(), [vec![129, 60, 0]]);
    assert_eq!(player.stop(25_000).unwrap(), [vec![128, 60, 0]]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn captured_banks_use_their_configured_channel_for_wire_and_notifications() {
    let mut player = player(
        "channel=3\nbank={kind='history'}\nexpression={source='recorded',timing='fit',gaps='carry'}",
        ClockMode::Internal,
    );
    for (at, data) in [
        (0, [176, 2, 30]),
        (0, [144, 60, 81]),
        (50_000, [128, 60, 20]),
    ] {
        player.accept(at, &data, InputSource::Both).unwrap();
    }
    assert_eq!(
        player.advance(225_000).unwrap(),
        [vec![178, 2, 30], vec![146, 60, 81], vec![130, 60, 20]]
    );
    let batch = player.take_events();
    assert!(batch.notes.iter().all(|n| n.channel == 3));
    assert!(batch.events.iter().all(|e| e.channel == 3));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn strum_capacity_fails_before_queuing_any_new_attacks() {
    let mut player = player(
        "strum={spacing='1/128 beat'}\nrhythm={kind='pattern',steps=[{kind='hit',duration='1 beat',repeats=5000}]}",
        ClockMode::Internal,
    );
    player.accept(0, &[144, 60, 90], InputSource::Both).unwrap();
    assert!(
        player
            .advance(0)
            .unwrap_err()
            .contains("4096 pending-attack")
    );
    assert!(player.stop(0).unwrap().is_empty());
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn lane_profiles_reject_duplicate_channels_nested_lanes_and_invalid_strums() {
    for body in [
        "channel=0",
        "channel=17",
        "channel=true",
        "strum={spacing='0 beat'}",
        "strum={spacing='-1 beat'}",
        "strum={spacing='1/8 beat',unknown=1}",
        "strum={spacing='1/8 beat'}\nselection={kind='index_pattern',indices=[0]}",
        "[lanes.bass]\nrhythm={kind='grid',step='1/3 beat'}",
        "[lanes.bass]\nchannel=2\nbank={kind='history'}\nrhythm={kind='grid',step='1/3 beat'}\nexpression={source='recorded',timing='fit',gaps='carry'}",
        "[lanes.bass]\nchannel=2\nrhythm={kind='grid',step='1/3 beat'}\n[lanes.bass.lanes.nested]\nchannel=3\nrhythm={kind='grid',step='1/3 beat'}",
    ] {
        let mut document: toml::Value = toml::from_str(
            "name='invalid'\ntitle='Invalid'\n[body]\nrhythm={kind='grid',step='1/4 beat'}",
        )
        .unwrap();
        document["body"]
            .as_table_mut()
            .unwrap()
            .extend(toml::from_str::<toml::Table>(body).unwrap());
        assert!(
            parse_profile(&toml::to_string(&document).unwrap(), None).is_err(),
            "{body}"
        );
    }
}

fn player(body: &str, mode: ClockMode) -> MidiPlayer {
    let mut document: toml::Value = toml::from_str("name='strums-lanes'\ntitle='Strums and lanes'\n[body]\nrhythm={kind='grid',step='1/4 beat'}").unwrap();
    document["body"]
        .as_table_mut()
        .unwrap()
        .extend(toml::from_str::<toml::Table>(body).unwrap());
    MidiPlayer::new(
        parse_profile(&toml::to_string(&document).unwrap(), None).unwrap(),
        mode,
        120,
        500_000,
    )
    .unwrap()
}
