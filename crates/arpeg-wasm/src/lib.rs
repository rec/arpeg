//! Browser bindings to the existing profile parser and MIDI player.

use arpeg_core::{
    clock::ClockMode,
    ports::{InputPort, OutputPort},
};
use arpeg_midi::{parse_profile, player::InputSource};
use js_sys::{Array, BigInt, Object, Reflect, Uint8Array};
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

    /// Record, commit a replacement, commit an overdub, or undo the last committed take.
    pub fn capture(&mut self, at_us: i64, command: &str) -> Result<Array, JsValue> {
        self.player
            .capture(at_us, command)
            .map(messages)
            .map_err(JsValue::from_str)
    }

    pub fn control(&mut self, at_us: i64, port: &str, value: &str) -> Result<Array, JsValue> {
        let port = match port {
            "gate" => InputPort::Gate,
            "density" => InputPort::Density,
            "transposition" => InputPort::Transposition,
            "selection_offset" => InputPort::SelectionOffset,
            "breath" => InputPort::Breath,
            "bend" => InputPort::Bend,
            "pressure" => InputPort::Pressure,
            _ => {
                return Err(JsValue::from_str("unsupported arpeggiator control port"));
            }
        };
        let value = value
            .parse()
            .map_err(|_| JsValue::from_str("control value must be an exact rational"))?;
        self.player
            .control(at_us, port, value)
            .map(messages)
            .map_err(JsValue::from_str)
    }

    pub fn take_events(&mut self) -> Result<JsValue, JsValue> {
        let batch = self.player.take_events();
        let events = Array::new();
        for event in batch.events {
            let object = Object::new();
            let port = match event.port {
                OutputPort::Step => "step",
                OutputPort::Hit => "hit",
                OutputPort::Rest => "rest",
            };
            for (key, value) in [
                ("at", JsValue::from_str(&event.at.to_string())),
                ("port", JsValue::from_str(port)),
                ("index", BigInt::from(event.index).into()),
                ("revision", BigInt::from(event.revision).into()),
            ] {
                Reflect::set(&object, &JsValue::from_str(key), &value)?;
            }
            events.push(&object);
        }
        let result = Object::new();
        Reflect::set(&result, &JsValue::from_str("events"), &events)?;
        Reflect::set(
            &result,
            &JsValue::from_str("exhausted"),
            &JsValue::from_bool(batch.exhausted),
        )?;
        Ok(result.into())
    }

    /// Recording flag, published note count, and bank revision, or an empty array for held banks.
    #[wasm_bindgen(getter)]
    pub fn capture_state(&self) -> Array {
        match self.player.capture_state() {
            Some((recording, notes, revision)) => [
                JsValue::from_bool(recording),
                JsValue::from_f64(notes as f64),
                JsValue::from_f64(revision as f64),
            ]
            .into_iter()
            .collect(),
            None => Array::new(),
        }
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
