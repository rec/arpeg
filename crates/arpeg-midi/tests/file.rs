use arpeg_midi::{Profile, parse_profile, render_file};
use midly::{
    Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
    num::{u4, u7, u15, u24, u28},
};

#[test]
fn file_render_preserves_tempo_and_emits_classic_note_order() {
    let profile = include_str!("../../../conformance/up.toml");
    parse_profile(profile).expect("supported profile");
    let channel = u4::from(0);
    let mut track = vec![TrackEvent {
        delta: u28::from(0),
        kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::from(500_000))),
    }];
    for key in [60, 64, 67] {
        track.push(TrackEvent {
            delta: u28::from(0),
            kind: TrackEventKind::Midi {
                channel,
                message: MidiMessage::NoteOn {
                    key: u7::from(key),
                    vel: u7::from(100),
                },
            },
        });
    }
    for (index, key) in [60, 64, 67].into_iter().enumerate() {
        track.push(TrackEvent {
            delta: u28::from(if index == 0 { 960 } else { 0 }),
            kind: TrackEventKind::Midi {
                channel,
                message: MidiMessage::NoteOff {
                    key: u7::from(key),
                    vel: u7::from(20),
                },
            },
        });
    }
    track.push(TrackEvent {
        delta: u28::from(0),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });
    let input = Smf {
        header: Header::new(Format::SingleTrack, Timing::Metrical(u15::from(480))),
        tracks: vec![track],
    };
    let mut bytes = Vec::new();
    input.write_std(&mut bytes).expect("input MIDI file");

    let output = render_file(profile, &bytes).expect("render MIDI file");
    let output = Smf::parse(&output).expect("valid output MIDI file");
    let mut tick = 0;
    let mut onsets = Vec::new();
    let mut releases = Vec::new();
    let mut tempos = Vec::new();
    for event in &output.tracks[0] {
        tick += event.delta.as_int();
        match event.kind {
            TrackEventKind::Midi {
                message: MidiMessage::NoteOn { key, vel },
                ..
            } if vel.as_int() > 0 => onsets.push((tick, key.as_int())),
            TrackEventKind::Midi {
                message: MidiMessage::NoteOff { key, vel },
                ..
            } => {
                assert_eq!(vel.as_int(), 20);
                releases.push((tick, key.as_int()));
            }
            TrackEventKind::Meta(MetaMessage::Tempo(value)) => {
                tempos.push((tick, value.as_int()));
            }
            _ => {}
        }
    }
    assert_eq!(tempos, [(0, 500_000)]);
    assert_eq!(
        onsets,
        [
            (0, 60),
            (120, 64),
            (240, 67),
            (360, 60),
            (480, 64),
            (600, 67),
            (720, 60),
            (840, 64)
        ]
    );
    assert_eq!(
        releases,
        [
            (96, 60),
            (216, 64),
            (336, 67),
            (456, 60),
            (576, 64),
            (696, 67),
            (816, 60),
            (936, 64)
        ]
    );
}

#[test]
fn incomplete_walk_profile_fails_explicitly() {
    let profile = include_str!("../../../conformance/up.toml").replace(
        "selection = { kind = \"ascending\" }",
        "selection = { kind = \"walk\" }",
    );
    assert!(parse_profile(&profile).is_err());
}

#[test]
fn alternating_profile_validates_endpoint_policy_and_is_live_only() {
    let profile = include_str!("../../../conformance/alternating.toml");
    for (policy, expected) in [
        ("", false),
        (", repeat_endpoints = false", false),
        (", repeat_endpoints = true", true),
    ] {
        let text = profile.replace(", repeat_endpoints = false", policy);
        let Profile::Classic(parsed) = parse_profile(&text).unwrap() else {
            panic!("expected live note profile");
        };
        assert_eq!(
            parsed.selection,
            arpeg_core::Selection::Alternating {
                repeat_endpoints: expected
            }
        );
    }
    assert_eq!(
        render_file(profile, &single_note_input(0)).unwrap_err(),
        "alternating selection currently requires live input"
    );
    for value in ["1", "\"true\""] {
        let text = profile.replace(
            "repeat_endpoints = false",
            &format!("repeat_endpoints = {value}"),
        );
        assert!(parse_profile(&text).is_err());
    }
    let history = profile.replace("[body]", "[body]\nbank = { kind = \"history\" }");
    assert!(parse_profile(&history).is_err());
}

#[test]
fn center_edge_profiles_are_live_only_and_have_no_extra_options() {
    for (profile, selection) in [
        (
            include_str!("../../../conformance/inside-out.toml"),
            arpeg_core::Selection::InsideOut,
        ),
        (
            include_str!("../../../conformance/outside-in.toml"),
            arpeg_core::Selection::OutsideIn,
        ),
    ] {
        let Profile::Classic(parsed) = parse_profile(profile).unwrap() else {
            panic!("expected live note profile");
        };
        assert_eq!(parsed.selection, selection);
        assert_eq!(
            render_file(profile, &single_note_input(0)).unwrap_err(),
            "center/edge selection currently requires live input"
        );
        let history = profile.replace("[body]", "[body]\nbank = { kind = \"history\" }");
        assert!(parse_profile(&history).is_err());
        let extra = profile.replace("selection = {", "selection = { repeats = 2,");
        assert!(parse_profile(&extra).is_err());
    }
}

#[test]
fn weighted_walk_profile_validates_choices_and_requires_live_input() {
    let profile = include_str!("../../../conformance/weighted-walk.toml");
    let Profile::Classic(parsed) = parse_profile(profile).unwrap() else {
        panic!("expected live note profile");
    };
    let arpeg_core::Selection::Walk(walk) = parsed.selection else {
        panic!("expected walk selection");
    };
    assert!(!walk.start_move);
    assert!(!walk.keep_rank);
    let changed = profile
        .replace("start = \"lowest\"", "start = \"move\"")
        .replace("on_remove = \"lowest\"", "on_remove = \"rank\"");
    let Profile::Classic(parsed) = parse_profile(&changed).unwrap() else {
        panic!("expected live note profile");
    };
    let arpeg_core::Selection::Walk(walk) = parsed.selection else {
        panic!("expected walk selection");
    };
    assert!(walk.start_move);
    assert!(walk.keep_rank);
    assert_eq!(
        render_file(profile, &single_note_input(0)).unwrap_err(),
        "probability currently requires live input"
    );
    for (original, replacement) in [
        ("seed = 42", ""),
        ("probability = \"2/3\"", "probability = \"4/3\""),
        ("weights = [1, 1, 3]", "weights = [1, 0, 3]"),
        ("weights = [1, 1, 3]", "weights = [1, 3]"),
        ("start = \"lowest\"", "start = \"unknown\""),
        ("on_remove = \"lowest\"", "on_remove = \"unknown\""),
    ] {
        assert!(parse_profile(&profile.replace(original, replacement)).is_err());
    }
}

#[test]
fn euclidean_profile_renders_hits_with_independent_releases() {
    let profile = include_str!("../../../conformance/euclidean.toml");
    let output = render_file(profile, &single_note_input(0)).unwrap();
    let output = Smf::parse(&output).unwrap();
    let mut tick = 0;
    let mut notes = Vec::new();
    for event in &output.tracks[0] {
        tick += event.delta.as_int();
        if let TrackEventKind::Midi {
            message: MidiMessage::NoteOn { vel, .. },
            ..
        } = event.kind
        {
            notes.push((tick, vel.as_int() > 0));
        }
    }
    assert_eq!(notes, [(0, true), (96, false), (360, true), (456, false)]);
}

#[test]
fn euclidean_profile_rejects_invalid_masks_and_history_use() {
    let profile = include_str!("../../../conformance/euclidean.toml");
    for (original, replacement) in [
        ("steps = 8", "steps = 0"),
        ("pulses = 3", "pulses = -1"),
        ("pulses = 3", "pulses = 9"),
        ("rotation = 0", "rotation = 0.5"),
        ("1/4 beat", "0 beat"),
    ] {
        assert!(parse_profile(&profile.replace(original, replacement)).is_err());
    }
    let Profile::Classic(parsed) = parse_profile(&profile.replace(", rotation = 0", "")).unwrap()
    else {
        panic!("expected classic profile");
    };
    assert_eq!(
        parsed.rhythm,
        arpeg_core::rhythm::Rhythm::Euclidean {
            step: arpeg_core::Beat::new(1, 4),
            steps: 8,
            pulses: 3,
            rotation: 0,
        }
    );
    let history = include_str!("../../../conformance/history-wind.toml").replace(
        "kind = \"grid\", step = \"1/4 beat\"",
        "kind = \"euclidean\", step = \"1/4 beat\", steps = 8, pulses = 3",
    );
    assert_eq!(
        parse_profile(&history).err().unwrap(),
        "history playback currently requires grid rhythm"
    );
}

#[test]
fn custom_pattern_profile_is_live_only_and_validates_steps() {
    let profile = include_str!("../../../conformance/custom-steps.toml");
    let Profile::Classic(parsed) = parse_profile(profile).unwrap() else {
        panic!("expected classic profile");
    };
    let decision = parsed.rhythm.decide_step(0, parsed.gate);
    assert_eq!(decision.duration, arpeg_core::Beat::new(1, 4));
    assert_eq!(decision.final_gate, arpeg_core::Beat::new(9, 20));
    assert_eq!(
        render_file(profile, &single_note_input(0)).unwrap_err(),
        "pattern rhythm currently requires live input"
    );
    for (original, replacement) in [
        ("1/4 beat", "0 beat"),
        ("repeats = 3", "repeats = 0"),
        ("repeats = 3", "repeats = 1.5"),
        (
            "kind = \"tie\", duration = \"1/4 beat\"",
            "kind = \"tie\", duration = \"1/4 beat\", repeats = 2",
        ),
        ("kind = \"rest\"", "kind = \"unknown\""),
    ] {
        assert!(parse_profile(&profile.replace(original, replacement)).is_err());
    }
    let empty = "kind = 'arpeggiator'\nname = 'empty'\ntitle = 'Empty'\n[body]\nrhythm = { kind = 'pattern', steps = [] }";
    assert!(parse_profile(empty).is_err());
}

#[test]
fn bank_edit_retrigger_is_live_only() {
    let profile = include_str!("../../../conformance/up.toml")
        .replace("[body]", "[body]\nretrigger = \"bank_edit\"");
    parse_profile(&profile).expect("supported live profile");
    assert_eq!(
        render_file(&profile, &single_note_input(0)).unwrap_err(),
        "file rendering does not support bank-edit retrigger"
    );
}

#[test]
fn native_shell_accepts_the_canonical_ufor_profile() {
    parse_profile(include_str!("../../../conformance/up-expanded.toml"))
        .expect("uFor-serialized profile");
    parse_profile(include_str!("../../../conformance/live-latch.toml"))
        .expect("live latch profile");
}

#[test]
fn history_profile_is_explicit_and_live_only() {
    let profile = include_str!("../../../conformance/history-wind.toml");
    let Profile::History(history) = parse_profile(profile).unwrap() else {
        panic!("history profile was parsed as classic");
    };
    assert_eq!(history.notes, 8);
    assert_eq!(history.selection, arpeg_core::Selection::Ascending);
    assert_eq!(
        render_file(profile, &single_note_input(0)).unwrap_err(),
        "history profiles require live MIDI input"
    );
    let unsupported = profile.replace("source = \"recorded\"", "source = \"current\"");
    assert!(parse_profile(&unsupported).is_err());
}

#[test]
fn velocity_zero_releases_keep_their_wire_encoding() {
    let bytes = single_note_input(0);
    let output = render_file(include_str!("../../../conformance/up.toml"), &bytes)
        .expect("render MIDI file");
    let output = Smf::parse(&output).expect("valid output MIDI file");
    let mut releases = 0;
    for event in &output.tracks[0] {
        match event.kind {
            TrackEventKind::Midi {
                message: MidiMessage::NoteOn { vel, .. },
                ..
            } if vel.as_int() == 0 => releases += 1,
            TrackEventKind::Midi {
                message: MidiMessage::NoteOff { .. },
                ..
            } => panic!("release encoding changed"),
            _ => {}
        }
    }
    assert_eq!(releases, 4);
}

#[test]
fn latched_file_keeps_playing_after_source_release() {
    let profile = include_str!("../../../conformance/up.toml").replace(
        "[body]",
        "[body]\nbank = { kind = \"latched\", update = \"replace\" }",
    );
    let bytes = single_note_input(480);
    let output = render_file(&profile, &bytes).expect("render latched MIDI file");
    let output = Smf::parse(&output).expect("valid output MIDI file");
    let onsets = output.tracks[0]
        .iter()
        .filter(|event| {
            matches!(event.kind, TrackEventKind::Midi {
                message: MidiMessage::NoteOn { vel, .. }, ..
            } if vel.as_int() > 0)
        })
        .count();
    assert_eq!(onsets, 8);
}

fn single_note_input(end_delay: u32) -> Vec<u8> {
    let channel = u4::from(0);
    let key = u7::from(60);
    let input = Smf {
        header: Header::new(Format::SingleTrack, Timing::Metrical(u15::from(480))),
        tracks: vec![vec![
            TrackEvent {
                delta: u28::from(0),
                kind: TrackEventKind::Midi {
                    channel,
                    message: MidiMessage::NoteOn {
                        key,
                        vel: u7::from(64),
                    },
                },
            },
            TrackEvent {
                delta: u28::from(480),
                kind: TrackEventKind::Midi {
                    channel,
                    message: MidiMessage::NoteOn {
                        key,
                        vel: u7::from(0),
                    },
                },
            },
            TrackEvent {
                delta: u28::from(end_delay),
                kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
            },
        ]],
    };
    let mut bytes = Vec::new();
    input.write_std(&mut bytes).expect("input MIDI file");
    bytes
}
