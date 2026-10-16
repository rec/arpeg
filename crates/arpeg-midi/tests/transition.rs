use arpeg_core::{Beat, clock::ClockMode, ports::NotePort};
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn transitions_match_python_at_irregular_polling_intervals() {
    let fixture: toml::Value =
        toml::from_str(include_str!("../../../conformance/transitions.toml")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let mut profile: toml::Value =
            toml::from_str(include_str!("../../../conformance/transition.toml")).unwrap();
        let body = profile["body"].as_table_mut().unwrap();
        body.insert("selection".into(), case["selection"].clone());
        if let Some(rhythm) = case.get("rhythm") {
            body.insert("rhythm".into(), rhythm.clone());
        }
        if let Some(patterns) = case.get("octaves") {
            body.insert("octaves".into(), patterns.clone());
        }
        for polling in [false, true] {
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
            let through: Beat = case["through"].as_str().unwrap().parse().unwrap();
            let end = (through * 500_000).to_integer();
            let times: Vec<_> = if polling {
                (0..end).step_by(17_000).chain([end]).collect()
            } else {
                vec![end]
            };
            let mut events = Vec::new();
            for time in times {
                player.advance(time).unwrap();
                events.extend(player.take_events().notes.into_iter().map(|e| {
                    toml::Value::Array(vec![
                        toml::Value::String(e.at.to_string()),
                        toml::Value::String(
                            if e.port == NotePort::NoteStart {
                                "on"
                            } else {
                                "off"
                            }
                            .into(),
                        ),
                        toml::Value::Integer(i64::from(e.key)),
                        toml::Value::Integer(i64::from(e.velocity)),
                    ])
                }));
            }
            assert_eq!(
                toml::Value::Array(events),
                case["events"],
                "{}",
                case["name"]
            );
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn transition_tables_require_valid_weights_policies_and_a_seed() {
    for options in [
        "weights=[]",
        "weights=[[-1]]",
        "weights=[[true]]",
        "weights=[[1.5]]",
        "weights=[[4294967296]]",
        "weights=[1]",
        "weights=[[1]],missing_weight=-1",
        "weights=[[1]],missing_weight=true",
        "weights=[[1]],missing_weight=4294967296",
        "weights=[[1]],start=\"middle\"",
        "weights=[[1]],blocked=\"repeat\"",
        "weights=[[1]],unknown=1",
    ] {
        let profile = format!(
            "name=\"transition\"\ntitle=\"Transition\"\n[body]\nseed=42\nrhythm={{kind=\"grid\",step=\"1/4 beat\"}}\nselection={{kind=\"transition\",{options}}}"
        );
        assert!(parse_profile(&profile, None).is_err(), "{options}");
    }
    let profile = include_str!("../../../conformance/transition.toml");
    assert!(parse_profile(&profile.replace("seed = 42", ""), None).is_err());
    let captured = profile.replace("[body]", "[body]\nbank={kind=\"history\"}");
    assert!(parse_profile(&captured, None).is_err());
}
