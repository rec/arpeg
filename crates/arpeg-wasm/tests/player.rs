#![cfg(target_arch = "wasm32")]

use arpeg_wasm::MidiPlayer;
use js_sys::Uint8Array;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn browser_player_returns_midi_bytes_and_exact_transport_state() {
    let mut player = MidiPlayer::new(
        include_str!("../../../conformance/up.toml"),
        "internal",
        120,
        500_000,
    )
    .unwrap();
    assert_eq!(
        player.accept(0, &[0x90, 60, 100], "both").unwrap().length(),
        0
    );
    let output = player.advance(0).unwrap();
    assert_eq!(Uint8Array::new(&output.get(0)).to_vec(), [0x90, 60, 100]);
    assert_eq!(player.beat(), "0");
    assert!(player.active());
    let output = player.advance(100_000).unwrap();
    assert_eq!(Uint8Array::new(&output.get(0)).to_vec(), [0x80, 60, 0]);
    assert_eq!(player.beat(), "1/5");
    let output = player.advance(125_000).unwrap();
    assert_eq!(Uint8Array::new(&output.get(0)).to_vec(), [0x90, 60, 100]);
    let output = player.stop(150_000).unwrap();
    assert_eq!(Uint8Array::new(&output.get(0)).to_vec(), [0x80, 60, 0]);
    assert!(!player.active());
}

#[wasm_bindgen_test]
fn browser_player_rejects_incomplete_messages_without_losing_state() {
    let mut player = MidiPlayer::new(
        include_str!("../../../conformance/up.toml"),
        "internal",
        120,
        500_000,
    )
    .unwrap();
    for data in [&[][..], &[0xb0][..], &[0x90, 60][..]] {
        assert!(player.accept(0, data, "both").is_err());
    }
    player.accept(0, &[0x90, 60, 100], "both").unwrap();
    assert_eq!(
        Uint8Array::new(&player.advance(0).unwrap().get(0)).to_vec(),
        [0x90, 60, 100]
    );
}
