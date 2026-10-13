use arpeg_core::{clock::ClockMode, ports::NotePort};
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn dense_recorded_breath_keeps_equal_samples_and_cancels_them_on_stop() {
    let profile =
        parse_profile(include_str!("../../../conformance/phrase-wind.toml"), None).unwrap();
    let mut player = MidiPlayer::new(profile, ClockMode::Internal, 120, 500_000).unwrap();
    player.capture(0, "record").unwrap();
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    for tick in 1..=2000 {
        player
            .accept(tick * 1000, &[176, 2, 64], InputSource::Both)
            .unwrap();
    }
    player
        .accept(2_001_000, &[128, 60, 20], InputSource::Both)
        .unwrap();
    player.capture(2_002_000, "commit").unwrap();
    assert_eq!(player.advance(2_125_000).unwrap(), [vec![144, 60, 100]]);
    let breath = player.advance(2_175_000).unwrap();
    assert_eq!(breath.len(), 1000);
    assert!(breath.iter().all(|m| m == &[176, 2, 64]));
    assert_eq!(player.stop(2_175_000).unwrap(), [vec![128, 60, 0]]);
    assert!(player.advance(3_000_000).unwrap().is_empty());
    let notes = player.take_events().notes;
    assert_eq!(
        notes.iter().map(|n| n.port).collect::<Vec<_>>(),
        [NotePort::NoteStart, NotePort::NoteEnd]
    );
    assert_eq!(notes[0].occurrence, notes[1].occurrence);
}
