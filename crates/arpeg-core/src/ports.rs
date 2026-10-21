//! Step-published scalar inputs and bounded discrete Envelfo outputs.

use crate::Beat;
use crate::octaves::Octaves;
use crate::timing_ports::TimingPorts;
pub use crate::timing_ports::{InputPort, OutputPort, PortEvent};

use crate::tuning::{PreparedTuning, RealizedPitch, TUNING_ERROR};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PitchBoundary {
    #[default]
    Drop,
    Fold,
    Error,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScaleMapping {
    #[default]
    Nearest,
    Up,
    Down,
    Drop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScaleTransform {
    pub offset: i64,
    pub period: i64,
    pub intervals: Vec<i64>,
    pub degrees: i64,
    pub mapping: ScaleMapping,
}

impl ScaleTransform {
    pub fn map_pitch(&self, key: i128) -> Option<i128> {
        let period = i128::from(self.period);
        let relative = key - i128::from(self.offset);
        let octave = relative.div_euclid(period);
        let chroma = relative.rem_euclid(period);
        let size = self.intervals.len() as i128;
        let mut rank = self.intervals.partition_point(|v| i128::from(*v) <= chroma) as i128 - 1;
        let lower = if rank < 0 {
            i128::from(*self.intervals.last().unwrap()) - period
        } else {
            i128::from(self.intervals[rank as usize])
        };
        if lower != chroma {
            if self.mapping == ScaleMapping::Drop {
                return None;
            }
            let upper = if rank + 1 < size {
                i128::from(self.intervals[(rank + 1) as usize])
            } else {
                i128::from(self.intervals[0]) + period
            };
            if self.mapping == ScaleMapping::Up
                || self.mapping == ScaleMapping::Nearest && upper - chroma < chroma - lower
            {
                rank += 1;
            }
        }
        let degree = octave * size + rank + i128::from(self.degrees);
        Some(
            i128::from(self.offset)
                + degree.div_euclid(size) * period
                + i128::from(self.intervals[degree.rem_euclid(size) as usize]),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PitchRegister {
    low: u8,
    high: u8,
    boundary: PitchBoundary,
}

impl PitchRegister {
    pub fn folding(&self) -> bool {
        self.boundary == PitchBoundary::Fold
    }
    pub fn new(low: u8, high: u8, boundary: PitchBoundary) -> Result<Self, &'static str> {
        if low > high || high > 127 {
            return Err("register bounds must be ordered within MIDI range 0–127");
        }
        Ok(Self {
            low,
            high,
            boundary,
        })
    }
}

pub struct PortBatch {
    pub events: Vec<PortEvent>,
    pub notes: Vec<NoteEvent>,
    pub exhausted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotePort {
    NoteStart,
    NoteEnd,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteEvent {
    pub channel: u8,
    pub at: Beat,
    pub port: NotePort,
    pub occurrence: u64,
    pub source: String,
    pub key: u8,
    pub velocity: u8,
}

pub struct PerformancePorts {
    pub channel: u8,
    pub strum_spacing: Option<Beat>,
    pub timing: TimingPorts,
    pub inversion: i64,
    pub voicing: Vec<i64>,
    pub octaves: Octaves,
    pub tuning: Option<PreparedTuning>,
    pub scale: Option<ScaleTransform>,
    pub register_limits: Option<PitchRegister>,
    pub transposition: i64,
    pub pitch_boundary: PitchBoundary,
    pending_transposition: Option<i64>,
}

impl std::ops::Deref for PerformancePorts {
    type Target = TimingPorts;
    fn deref(&self) -> &TimingPorts {
        &self.timing
    }
}
impl std::ops::DerefMut for PerformancePorts {
    fn deref_mut(&mut self) -> &mut TimingPorts {
        &mut self.timing
    }
}

impl PerformancePorts {
    pub fn new(gate: Beat, density: Beat) -> Self {
        Self {
            channel: 0,
            strum_spacing: None,
            timing: TimingPorts::new(gate, density),
            inversion: 0,
            voicing: Vec::new(),
            octaves: Octaves::default(),
            tuning: None,
            scale: None,
            register_limits: None,
            transposition: 0,
            pitch_boundary: PitchBoundary::Drop,
            pending_transposition: None,
        }
    }
    pub fn check_control(
        port: InputPort,
        value: Beat,
        seed: Option<i64>,
    ) -> Result<(), &'static str> {
        if port == InputPort::Transposition {
            return if value.is_integer() {
                Ok(())
            } else {
                Err("transposition and selection offset require whole numbers")
            };
        }
        TimingPorts::check_control(port, value, seed)
    }
    pub fn queue(&mut self, port: InputPort, value: Beat) -> Result<(), &'static str> {
        if port == InputPort::Transposition {
            self.pending_transposition = Some(value.to_integer());
            Ok(())
        } else {
            self.timing.queue(port, value)
        }
    }
    pub fn cancel_pending(&mut self) {
        self.pending_transposition = None;
        self.timing.cancel_pending();
    }
    pub fn begin_step(
        &mut self,
        at: Beat,
        index: i64,
        revision: u64,
        capture_ready: bool,
        cycle: bool,
    ) -> bool {
        if let Some(value) = self.pending_transposition.take() {
            self.transposition = value;
        }
        self.timing
            .begin_step(at, index, revision, capture_ready, cycle)
    }
    pub fn take_events(&mut self) -> PortBatch {
        let batch = self.timing.take_events();
        PortBatch {
            events: batch.events,
            notes: Vec::new(),
            exhausted: batch.exhausted,
        }
    }
    pub fn realize_pitch(&self, key: i128) -> Result<Option<RealizedPitch>, &'static str> {
        let key = match &self.scale {
            Some(scale) => match scale.map_pitch(key) {
                Some(pitch) => pitch,
                None => return Ok(None),
            },
            None => key,
        };
        if let Some(tuning) = &self.tuning {
            let mut pitch = tuning.pitch(key)? + self.transposition as f64;
            if !pitch.is_finite() {
                return Err(TUNING_ERROR);
            }
            let (low, high, boundary) = self
                .register_limits
                .map_or((0.0, 127.0, self.pitch_boundary), |r| {
                    (f64::from(r.low), f64::from(r.high), r.boundary)
                });
            if pitch < low || pitch > high {
                match boundary {
                    PitchBoundary::Drop => return Ok(None),
                    PitchBoundary::Error => return Err(TUNING_ERROR),
                    PitchBoundary::Fold => {
                        let span = tuning.period_semitones.abs();
                        if span == 0.0 {
                            return Ok(None);
                        }
                        pitch = if pitch < low {
                            pitch + ((low - pitch) / span).ceil() * span
                        } else {
                            pitch - ((pitch - high) / span).ceil() * span
                        };
                        if !(low..=high).contains(&pitch) {
                            return Ok(None);
                        }
                    }
                }
            }
            let base = (pitch + 0.5).floor().min(127.0) as u8;
            return Ok(Some(RealizedPitch {
                key: base,
                tuning_semitones: pitch - f64::from(base),
            }));
        }
        let pitch = key + i128::from(self.transposition);
        let (low, high, boundary) = self
            .register_limits
            .map_or((0, 127, self.pitch_boundary), |r| {
                (i128::from(r.low), i128::from(r.high), r.boundary)
            });
        if (low..=high).contains(&pitch) {
            return Ok(Some(RealizedPitch {
                key: pitch as u8,
                tuning_semitones: 0.0,
            }));
        }
        match boundary {
            PitchBoundary::Drop => Ok(None),
            PitchBoundary::Fold => {
                let folded = if pitch < low {
                    low + (pitch - low).rem_euclid(12)
                } else {
                    high - (high - pitch).rem_euclid(12)
                };
                Ok(if (low..=high).contains(&folded) {
                    Some(RealizedPitch {
                        key: folded as u8,
                        tuning_semitones: 0.0,
                    })
                } else {
                    None
                })
            }
            PitchBoundary::Error if self.register_limits.is_some() => Err(REGISTER_RANGE_ERROR),
            PitchBoundary::Error => Err("transposed pitch is outside MIDI range 0–127"),
        }
    }
}

pub const REGISTER_RANGE_ERROR: &str = "pitch is outside configured register limits";
