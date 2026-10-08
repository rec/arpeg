//! Browser bindings to the existing profile parser and MIDI player.

use arpeg_core::clock::ClockMode;
use arpeg_midi::{parse_profile, player::InputSource};
use js_sys::{Array, Uint8Array};
use wasm_bindgen::{JsValue, prelude::wasm_bindgen};

#[wasm_bindgen]
pub struct MidiPlayer {
    player: arpeg_midi::player::MidiPlayer,
}

#[wasm_bindgen]
impl MidiPlayer {
    #[wasm_bindgen(constructor)]
    pub fn new(
        profile: &str,
        clock: &str,
        bpm: u32,
        timeout_us: i64,
    ) -> Result<MidiPlayer, JsValue> {
        let mode = match clock {
            "internal" => ClockMode::Internal,
            "external" => ClockMode::External,
            _ => return Err(JsValue::from_str("clock must be internal or external")),
        };
        let profile = parse_profile(profile, None).map_err(|e| JsValue::from_str(&e))?;
        let player = arpeg_midi::player::MidiPlayer::new(profile, mode, bpm, timeout_us)
            .map_err(JsValue::from_str)?;
        Ok(Self { player })
    }

    /// Accept one complete MIDI message, including its status byte.
    pub fn accept(&mut self, at_us: i64, data: &[u8], source: &str) -> Result<Array, JsValue> {
        midly::live::LiveEvent::parse(data)
            .map_err(|e| JsValue::from_str(&format!("invalid MIDI message: {e}")))?;
        let source = match source {
            "notes" => InputSource::Notes,
            "clock" => InputSource::Clock,
            "both" => InputSource::Both,
            _ => return Err(JsValue::from_str("source must be notes, clock, or both")),
        };
        self.player
            .accept(at_us, data, source)
            .map(messages)
            .map_err(JsValue::from_str)
    }

    pub fn advance(&mut self, at_us: i64) -> Result<Array, JsValue> {
        self.player
            .advance(at_us)
            .map(messages)
            .map_err(JsValue::from_str)
    }

    pub fn clear(&mut self, at_us: i64) -> Result<Array, JsValue> {
        self.player
            .clear(at_us)
            .map(messages)
            .map_err(JsValue::from_str)
    }

    pub fn stop(&mut self, at_us: i64) -> Result<Array, JsValue> {
        self.player
            .stop(at_us)
            .map(messages)
            .map_err(JsValue::from_str)
    }

    pub fn set_tempo(&mut self, at_us: i64, bpm: u32) -> Result<Array, JsValue> {
        self.player
            .set_tempo(at_us, bpm)
            .map(messages)
            .map_err(JsValue::from_str)
    }

    #[wasm_bindgen(getter)]
    pub fn beat(&self) -> String {
        self.player.clock.beat.to_string()
    }

    #[wasm_bindgen(getter)]
    pub fn active(&self) -> bool {
        self.player.clock.active()
    }
}

fn messages(output: Vec<Vec<u8>>) -> Array {
    output
        .iter()
        .map(|data| Uint8Array::from(data.as_slice()))
        .collect()
}
