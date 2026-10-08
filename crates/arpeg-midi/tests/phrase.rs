use arpeg_core::clock::ClockMode;
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};

fn player() -> MidiPlayer {
    MidiPlayer::new(
        parse_profile(include_str!("../../../conformance/phrase-wind.toml"), None).unwrap(),
        ClockMode::Internal,
        120,
        500_000,
    )
    .unwrap()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn invalid_capture_controls_preserve_pending_output() {
    let mut player = player();
    for command in ["commit", "overdub", "undo", "unknown"] {
        assert!(player.capture(100_000, command).is_err());
        assert_eq!(player.clock.beat.to_string(), "0");
        assert_eq!(player.capture_state(), Some((false, 0, 0)));
    }
    player.capture(0, "record").unwrap();
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    assert!(player.capture(100_000, "record").is_err());
    assert_eq!(player.clock.beat.to_string(), "0");
    player.capture(0, "commit").unwrap();
    player.advance(0).unwrap();
    assert!(player.capture(100_000, "commit").is_err());
    assert_eq!(player.advance(100_000).unwrap(), [vec![128, 60, 0]]);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn take_limit_can_be_recovered_with_undo_and_clear() {
    let mut player = player();
    for _ in 0..128 {
        player.capture(0, "record").unwrap();
        player.capture(0, "overdub").unwrap();
    }
    player.capture(0, "record").unwrap();
    assert_eq!(
        player.capture(0, "commit").unwrap_err(),
        "phrase reached its 128-take limit; undo or clear first"
    );
    player.capture(0, "undo").unwrap();
    player.capture(0, "commit").unwrap();
    player.capture(0, "undo").unwrap();
    player.capture(0, "record").unwrap();
    player.clear(0).unwrap();
    player.capture(0, "record").unwrap();
    player.accept(0, &[144, 64, 90], InputSource::Both).unwrap();
    player.capture(0, "commit").unwrap();
    assert_eq!(player.advance(125_000).unwrap(), [vec![144, 64, 90]]);
}
