//! Device-independent MIDI player shared by host execution and conformance tests.

use arpeg_core::{
    capture::{MidiEvent, Profile as CaptureProfile},
    clock::{ClockMode, TransportClock},
    gesture::RealizedEvent,
    history::HistoryArpeggiator,
    live::{LiveArpeggiator, OutputEvent, OutputKind},
};

use crate::Profile;

#[derive(Clone, Copy)]
pub enum InputSource {
    Notes,
    Clock,
    Both,
}

enum Engine {
    Held(Box<LiveArpeggiator>),
    History(Box<HistoryArpeggiator>),
}

pub struct MidiPlayer {
    pub clock: TransportClock,
    engine: Engine,
    origin_us: Option<i64>,
    wall_us: i64,
    input_tick: i64,
    ordinal: u32,
    current_expression: bool,
    expression: std::collections::BTreeMap<u8, Vec<u8>>,
    sounding: Option<(u64, u8)>,
}

impl MidiPlayer {
    pub fn new(
        profile: Profile,
        mode: ClockMode,
        bpm: u32,
        timeout_us: i64,
    ) -> Result<Self, &'static str> {
        let current_expression = match &profile {
            Profile::Classic(_) => true,
            Profile::Captured(p) => p.current_expression,
        };
        let engine = match profile {
            Profile::Classic(p) => Engine::Held(Box::new(LiveArpeggiator::new(
                p.bank,
                p.selection,
                p.rhythm,
                p.gate,
                p.retrigger,
                p.chance,
            )?)),
            Profile::Captured(p) => Engine::History(Box::new(HistoryArpeggiator::new(
                p.mode,
                p.selection,
                p.retrigger == arpeg_core::live::Retrigger::BankEdit,
                p.step,
                p.gate,
                CaptureProfile::default(),
                p.current_expression,
            )?)),
        };
        Ok(Self {
            clock: TransportClock::new(mode, bpm, timeout_us)?,
            engine,
            origin_us: None,
            wall_us: 0,
            input_tick: -1,
            ordinal: 0,
            current_expression,
            expression: std::collections::BTreeMap::new(),
            sounding: None,
        })
    }

    pub fn accept(
        &mut self,
        at_us: i64,
        data: &[u8],
        source: InputSource,
    ) -> Result<Vec<Vec<u8>>, &'static str> {
        let transport = matches!(data[0], 0xf8 | 0xfa | 0xfb | 0xfc | 0xf2);
        if transport {
            if matches!(source, InputSource::Notes)
                || data[0] == 0xf8 && self.clock.mode == ClockMode::Internal
            {
                return Ok(Vec::new());
            }
            let tick = self.elapsed(at_us);
            if self.clock.accept(tick, data)? {
                let at = self.clock.beat;
                return Ok(match &mut self.engine {
                    Engine::Held(e) => {
                        let events = e.relocate(at);
                        self.note_messages(events)
                    }
                    Engine::History(e) => {
                        let events = e.relocate(at, tick);
                        self.history_messages(events)
                    }
                });
            }
            return Ok(if self.clock.active() {
                Vec::new()
            } else {
                self.pause(tick)
            });
        }
        if matches!(source, InputSource::Clock) {
            return Ok(Vec::new());
        }
        let expressive = matches!(data[0], 0xd0 | 0xe0) || data[0] == 0xb0 && data[1] == 2;
        if matches!(self.engine, Engine::Held(_))
            && !expressive
            && (data.len() != 3 || !matches!(data[0], 0x80 | 0x90))
        {
            return Ok(Vec::new());
        }
        let tick = self.elapsed(at_us);
        let was_active = self.clock.active();
        let at = self.clock.advance(tick);
        let mut output = if was_active && !self.clock.active() {
            self.pause(tick)
        } else {
            Vec::new()
        };
        match &mut self.engine {
            Engine::Held(e) => {
                if expressive {
                    let events = e.before(at)?;
                    output.extend(self.note_messages(events));
                    self.expression.insert(data[0], data.to_vec());
                    if self.clock.active() && self.sounding.is_some() {
                        output.push(data.to_vec());
                    }
                    return Ok(output);
                }
                let events = if data[0] == 0x90 && data[2] > 0 {
                    e.note_on(at, data[1], data[2])?
                } else {
                    e.note_off(at, data[1])?
                };
                output.extend(self.note_messages(events));
            }
            Engine::History(e) => {
                let tick = tick
                    .max(e.capture_to + i64::from(e.inclusive))
                    .max(self.input_tick);
                let events = if self.clock.active() && at > e.processed_to {
                    e.before(at, tick)?
                } else {
                    Vec::new()
                };
                if tick != self.input_tick {
                    self.input_tick = tick;
                    self.ordinal = 0;
                }
                e.accept(MidiEvent {
                    tick,
                    ordinal: self.ordinal,
                    data: data.to_vec(),
                })?;
                self.ordinal = self
                    .ordinal
                    .checked_add(1)
                    .ok_or("too many simultaneous MIDI messages")?;
                output.extend(self.history_messages(events));
                if expressive {
                    self.expression.insert(data[0], data.to_vec());
                    if self.current_expression && self.clock.active() && self.sounding.is_some() {
                        output.push(data.to_vec());
                    }
                }
            }
        }
        Ok(output)
    }

    pub fn advance(&mut self, at_us: i64) -> Result<Vec<Vec<u8>>, &'static str> {
        if self.origin_us.is_none() {
            return Ok(Vec::new());
        }
        let tick = self.elapsed(at_us);
        let was_active = self.clock.active();
        let at = self.clock.advance(tick);
        if was_active && !self.clock.active() {
            return Ok(self.pause(tick));
        }
        if !self.clock.active() {
            return Ok(Vec::new());
        }
        Ok(match &mut self.engine {
            Engine::Held(e) => {
                let events = e.advance(at)?;
                self.note_messages(events)
            }
            Engine::History(e) => {
                let events = e.advance(at, tick.max(self.input_tick))?;
                self.history_messages(events)
            }
        })
    }

    pub fn clear(&mut self, at_us: i64) -> Result<Vec<Vec<u8>>, &'static str> {
        let mut output = self.advance(at_us)?;
        let tick = self.elapsed(at_us);
        let at = self.clock.advance(tick);
        output.extend(match &mut self.engine {
            Engine::Held(e) => {
                let events = e.clear(at)?;
                self.note_messages(events)
            }
            Engine::History(e) => {
                let events = e.clear(at, tick)?;
                self.history_messages(events)
            }
        });
        Ok(output)
    }

    pub fn capture(&mut self, at_us: i64, command: &str) -> Result<Vec<Vec<u8>>, &'static str> {
        let Engine::History(engine) = &self.engine else {
            return Err("capture controls require a phrase bank");
        };
        engine.check_control(command)?;
        if command == "record" {
            engine.check_record_budget(self.expression.len())?;
        }
        let capture_to = engine.capture_to + i64::from(engine.inclusive);
        let tick = self.elapsed(at_us).max(self.input_tick + 1).max(capture_to);
        let was_active = self.clock.active();
        let at = self.clock.advance(self.wall_us);
        let mut output = if was_active && !self.clock.active() {
            self.pause(tick)
        } else {
            Vec::new()
        };
        let Engine::History(engine) = &mut self.engine else {
            unreachable!()
        };
        let events = engine.capture(command, at, tick)?;
        self.input_tick = tick;
        self.ordinal = 0;
        if command == "record" {
            for status in [0xb0, 0xe0, 0xd0] {
                if let Some(data) = self.expression.get(&status) {
                    engine.accept(MidiEvent {
                        tick,
                        ordinal: self.ordinal,
                        data: data.clone(),
                    })?;
                    self.ordinal += 1;
                }
            }
        }
        output.extend(self.history_messages(events));
        Ok(output)
    }

    pub fn capture_state(&self) -> Option<(bool, usize, usize)> {
        match &self.engine {
            Engine::History(e) => Some((
                e.bank.recording.is_some(),
                e.bank.published.len(),
                e.bank.revision,
            )),
            Engine::Held(_) => None,
        }
    }

    pub fn stop(&mut self, at_us: i64) -> Result<Vec<Vec<u8>>, &'static str> {
        let tick = self.elapsed(at_us);
        self.clock.halt(tick);
        Ok(self.pause(tick))
    }

    pub fn set_tempo(&mut self, at_us: i64, bpm: u32) -> Result<Vec<Vec<u8>>, &'static str> {
        if bpm == 0 || bpm > 1000 || self.clock.mode != ClockMode::Internal {
            return Err("tempo requires internal clock and BPM between 1 and 1000");
        }
        let output = self.advance(at_us)?;
        let tick = self.elapsed(at_us);
        self.clock.set_tempo(tick, bpm)?;
        Ok(output)
    }

    fn pause(&mut self, tick: i64) -> Vec<Vec<u8>> {
        match &mut self.engine {
            Engine::Held(e) => {
                let events = e.pause(self.clock.beat);
                self.note_messages(events)
            }
            Engine::History(e) => {
                let events = e.pause(self.clock.beat, tick);
                self.history_messages(events)
            }
        }
    }

    fn note_messages(&mut self, events: Vec<OutputEvent>) -> Vec<Vec<u8>> {
        let mut output = Vec::new();
        for event in events {
            match event.kind {
                OutputKind::NoteOn {
                    id, key, velocity, ..
                } => {
                    if let Some((_, key)) = self.sounding {
                        output.push(vec![0x80, key, 0]);
                    }
                    for status in [0xb0, 0xe0, 0xd0] {
                        if let Some(data) = self.expression.get(&status) {
                            output.push(data.clone());
                        }
                    }
                    output.push(vec![0x90, key, velocity]);
                    self.sounding = Some((id, key));
                }
                OutputKind::NoteOff { id, key, .. } if self.sounding.is_some_and(|s| s.0 == id) => {
                    output.push(vec![0x80, key, 0]);
                    self.sounding = None;
                }
                _ => {}
            }
        }
        output
    }

    fn history_messages(&mut self, events: Vec<RealizedEvent>) -> Vec<Vec<u8>> {
        let mut output = Vec::new();
        for event in events {
            let kind = event.data[0] & 0xf0;
            if kind == 0x90 && event.data[2] > 0 {
                if self.current_expression {
                    for status in [0xb0, 0xe0, 0xd0] {
                        if let Some(data) = self.expression.get(&status) {
                            output.push(data.clone());
                        }
                    }
                }
                self.sounding = Some((0, event.data[1]));
            } else if kind == 0x80 || kind == 0x90 && event.data[2] == 0 {
                self.sounding = None;
            }
            output.push(event.data);
        }
        output
    }

    fn elapsed(&mut self, at_us: i64) -> i64 {
        let origin = *self.origin_us.get_or_insert(at_us);
        self.clock.initialize(0);
        self.wall_us = self.wall_us.max(at_us - origin);
        self.wall_us
    }
}
