//! Device-independent MIDI player shared by host execution and conformance tests.

use arpeg_core::{
    capture::{MidiEvent, Profile as CaptureProfile},
    clock::{ClockMode, TransportClock},
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
    Held(LiveArpeggiator),
    History(HistoryArpeggiator),
}

pub struct MidiPlayer {
    pub clock: TransportClock,
    engine: Engine,
    origin_us: Option<i64>,
    wall_us: i64,
    input_tick: i64,
    ordinal: u32,
}

impl MidiPlayer {
    pub fn new(
        profile: Profile,
        mode: ClockMode,
        bpm: u32,
        timeout_us: i64,
    ) -> Result<Self, &'static str> {
        let engine = match profile {
            Profile::Classic(p) => Engine::Held(LiveArpeggiator::new(
                p.bank,
                p.selection,
                p.rhythm,
                p.gate,
                p.retrigger,
                p.chance,
            )?),
            Profile::History(p) => Engine::History(HistoryArpeggiator::new(
                p.notes,
                p.selection,
                p.retrigger == arpeg_core::live::Retrigger::BankEdit,
                p.step,
                p.gate,
                CaptureProfile::default(),
            )?),
        };
        Ok(Self {
            clock: TransportClock::new(mode, bpm, timeout_us)?,
            engine,
            origin_us: None,
            wall_us: 0,
            input_tick: -1,
            ordinal: 0,
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
                    Engine::Held(e) => note_messages(e.relocate(at)),
                    Engine::History(e) => {
                        e.relocate(at, tick).into_iter().map(|e| e.data).collect()
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
        if matches!(self.engine, Engine::Held(_))
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
                let events = if data[0] == 0x90 && data[2] > 0 {
                    e.note_on(at, data[1], data[2])?
                } else {
                    e.note_off(at, data[1])?
                };
                output.extend(note_messages(events));
            }
            Engine::History(e) => {
                let tick = tick
                    .max(e.capture_to + i64::from(e.inclusive))
                    .max(self.input_tick);
                if self.clock.active() && at > e.processed_to {
                    output.extend(e.before(at, tick)?.into_iter().map(|e| e.data));
                }
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
            Engine::Held(e) => note_messages(e.advance(at)?),
            Engine::History(e) => e
                .advance(at, tick.max(self.input_tick))?
                .into_iter()
                .map(|e| e.data)
                .collect(),
        })
    }

    pub fn clear(&mut self, at_us: i64) -> Result<Vec<Vec<u8>>, &'static str> {
        let mut output = self.advance(at_us)?;
        let tick = self.elapsed(at_us);
        let at = self.clock.advance(tick);
        output.extend(match &mut self.engine {
            Engine::Held(e) => note_messages(e.clear(at)?),
            Engine::History(e) => e.clear(at, tick)?.into_iter().map(|e| e.data).collect(),
        });
        Ok(output)
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
            Engine::Held(e) => note_messages(e.pause(self.clock.beat)),
            Engine::History(e) => e
                .pause(self.clock.beat, tick)
                .into_iter()
                .map(|e| e.data)
                .collect(),
        }
    }

    fn elapsed(&mut self, at_us: i64) -> i64 {
        let origin = *self.origin_us.get_or_insert(at_us);
        self.clock.initialize(0);
        self.wall_us = self.wall_us.max(at_us - origin);
        self.wall_us
    }
}

fn note_messages(events: Vec<OutputEvent>) -> Vec<Vec<u8>> {
    events
        .into_iter()
        .map(|e| match e.kind {
            OutputKind::NoteOn { key, velocity, .. } => vec![0x90, key, velocity],
            OutputKind::NoteOff { key, .. } => vec![0x80, key, 0],
        })
        .collect()
}
