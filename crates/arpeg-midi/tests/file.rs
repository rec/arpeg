use arpeg_midi::{parse_profile, render_file};
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
fn unsupported_profile_modes_fail_explicitly() {
    let profile = include_str!("../../../conformance/up.toml").replace(
        "selection = { kind = \"ascending\" }",
        "selection = { kind = \"walk\" }",
    );
    assert!(parse_profile(&profile).is_err());
}

#[test]
fn native_shell_accepts_the_canonical_ufor_profile() {
    parse_profile(include_str!("../../../conformance/up-expanded.toml"))
        .expect("uFor-serialized profile");
}

#[test]
fn velocity_zero_releases_keep_their_wire_encoding() {
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
                delta: u28::from(0),
                kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
            },
        ]],
    };
    let mut bytes = Vec::new();
    input.write_std(&mut bytes).expect("input MIDI file");
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
