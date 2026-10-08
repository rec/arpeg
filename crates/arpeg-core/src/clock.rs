//! Exact host transport with 24-pulse MIDI clock acquisition.

use crate::Beat;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockMode {
    Internal,
    External,
}

#[derive(Clone, Debug)]
pub struct ClockObservation {
    pub at_us: i64,
    pub data: Vec<u8>,
}

pub struct TransportClock {
    pub mode: ClockMode,
    pub bpm: u32,
    pub timeout_us: i64,
    pub beat: Beat,
    anchor_beat: Beat,
    anchor_us: Option<i64>,
    running: bool,
    last_pulse_us: Option<i64>,
    interval_us: Option<i64>,
    waiting: bool,
    pub observations: Vec<ClockObservation>,
}

impl TransportClock {
    pub fn new(mode: ClockMode, bpm: u32, timeout_us: i64) -> Result<Self, &'static str> {
        if bpm == 0 || bpm > 1000 || timeout_us <= 0 {
            return Err("invalid clock tempo or timeout");
        }
        Ok(Self {
            mode,
            bpm,
            timeout_us,
            beat: Beat::from_integer(0),
            anchor_beat: Beat::from_integer(0),
            anchor_us: None,
            running: mode == ClockMode::Internal,
            last_pulse_us: None,
            interval_us: None,
            waiting: true,
            observations: Vec::new(),
        })
    }

    pub fn active(&self) -> bool {
        self.running
            && self.anchor_us.is_some()
            && (self.mode == ClockMode::Internal || !self.waiting)
    }

    pub fn initialize(&mut self, at_us: i64) {
        if self.anchor_us.is_none() {
            self.anchor_us = Some(at_us);
        }
    }

    pub fn advance(&mut self, at_us: i64) -> Beat {
        if self.active() {
            let elapsed = (at_us - self.anchor_us.expect("active clock")).max(0);
            if self.mode == ClockMode::Internal {
                self.beat = self.anchor_beat + Beat::new(elapsed * i64::from(self.bpm), 60_000_000);
            } else {
                if let Some(interval) = self.interval_us {
                    self.beat = self.beat.max(
                        self.anchor_beat
                            + Beat::new(elapsed, interval).min(Beat::from_integer(1)) / 24,
                    );
                }
                if at_us - self.last_pulse_us.expect("acquired clock") >= self.timeout_us {
                    self.running = false;
                    self.waiting = true;
                }
            }
        }
        self.beat
    }

    pub fn accept(&mut self, at_us: i64, data: &[u8]) -> Result<bool, &'static str> {
        self.advance(at_us);
        if self.observations.len() >= 1_000_000 {
            return Err("clock observation limit reached");
        }
        self.observations.push(ClockObservation {
            at_us,
            data: data.to_vec(),
        });
        match data[0] {
            0xfa | 0xf2 => {
                self.beat = if data[0] == 0xfa {
                    Beat::from_integer(0)
                } else {
                    Beat::new(i64::from(data[1]) + 128 * i64::from(data[2]), 4)
                };
                self.anchor_beat = self.beat;
                self.anchor_us = Some(at_us);
                self.last_pulse_us = None;
                self.interval_us = None;
                self.waiting = true;
                if data[0] == 0xfa {
                    self.running = true;
                }
                Ok(true)
            }
            0xfc => {
                self.running = false;
                self.waiting = true;
                Ok(false)
            }
            0xfb => {
                self.running = true;
                self.anchor_us = Some(at_us);
                self.anchor_beat = self.beat;
                self.last_pulse_us = None;
                self.interval_us = None;
                self.waiting = true;
                Ok(false)
            }
            0xf8 if self.mode == ClockMode::External && self.running => {
                if self.last_pulse_us.is_some_and(|last| at_us <= last) {
                    return Ok(false);
                }
                if self.waiting {
                    self.anchor_beat = self.beat;
                    self.waiting = false;
                } else {
                    self.interval_us = Some(at_us - self.last_pulse_us.expect("acquired clock"));
                    self.anchor_beat += Beat::new(1, 24);
                    self.beat = self.beat.max(self.anchor_beat);
                }
                self.last_pulse_us = Some(at_us);
                self.anchor_us = Some(at_us);
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    pub fn set_tempo(&mut self, at_us: i64, bpm: u32) -> Result<(), &'static str> {
        if bpm == 0 || bpm > 1000 || self.mode != ClockMode::Internal {
            return Err("tempo requires internal clock and BPM between 1 and 1000");
        }
        self.advance(at_us);
        self.anchor_us = Some(at_us);
        self.anchor_beat = self.beat;
        self.bpm = bpm;
        Ok(())
    }

    pub fn halt(&mut self, at_us: i64) {
        self.advance(at_us);
        self.running = false;
        self.waiting = true;
    }
}
