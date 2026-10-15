//! Step-published scalar inputs and bounded discrete Envelfo outputs.

use crate::Beat;
use crate::tuning::{PreparedTuning, RealizedPitch, TUNING_ERROR};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputPort {
    Gate,
    Density,
    Transposition,
    SelectionOffset,
    Breath,
    Bend,
    Pressure,
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputPort {
    CaptureReady,
    Cycle,
    Step,
    Hit,
    Rest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortEvent {
    pub at: Beat,
    pub port: OutputPort,
    pub index: i64,
    pub revision: u64,
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
    pub at: Beat,
    pub port: NotePort,
    pub occurrence: u64,
    pub source: String,
    pub key: u8,
    pub velocity: u8,
}

pub struct PerformancePorts {
    pub tuning: Option<PreparedTuning>,
    pub scale: Option<ScaleTransform>,
    pub register_limits: Option<PitchRegister>,
    pub gate: Beat,
    pub density: Beat,
    pub transposition: i64,
    pub pitch_boundary: PitchBoundary,
    pub selection_offset: i64,
    pub offset_rest_outside: bool,
    pending_gate: Option<Beat>,
    pending_density: Option<Beat>,
    pending_transposition: Option<i64>,
    pending_selection_offset: Option<i64>,
    events: Vec<PortEvent>,
    exhausted: bool,
}

impl PerformancePorts {
    pub fn new(gate: Beat, density: Beat) -> Self {
        Self {
            scale: None,
            tuning: None,
            register_limits: None,
            gate,
            density,
            transposition: 0,
            pitch_boundary: PitchBoundary::Drop,
            selection_offset: 0,
            offset_rest_outside: false,
            pending_gate: None,
            pending_density: None,
            pending_transposition: None,
            pending_selection_offset: None,
            events: Vec::new(),
            exhausted: false,
        }
    }

    pub fn check_control(
        port: InputPort,
        value: Beat,
        seed: Option<i64>,
    ) -> Result<(), &'static str> {
        if matches!(
            port,
            InputPort::Breath | InputPort::Bend | InputPort::Pressure
        ) {
            return Err("expression controls require the MIDI player");
        }
        if matches!(port, InputPort::Transposition | InputPort::SelectionOffset) {
            return if value.is_integer() {
                Ok(())
            } else {
                Err("transposition and selection offset require whole numbers")
            };
        }
        if value < Beat::from_integer(0)
            || port == InputPort::Density && value > Beat::from_integer(1)
        {
            return Err("gate must be nonnegative and density must be between zero and one");
        }
        if port == InputPort::Density
            && value > Beat::from_integer(0)
            && value < Beat::from_integer(1)
            && seed.is_none()
        {
            return Err("density between zero and one requires an explicit seed");
        }
        Ok(())
    }

    pub fn queue(&mut self, port: InputPort, value: Beat) -> Result<(), &'static str> {
        match port {
            InputPort::Gate => self.pending_gate = Some(value),
            InputPort::Density => self.pending_density = Some(value),
            InputPort::Transposition => self.pending_transposition = Some(value.to_integer()),
            InputPort::SelectionOffset => self.pending_selection_offset = Some(value.to_integer()),
            InputPort::Breath | InputPort::Bend | InputPort::Pressure => {
                return Err("expression controls require the MIDI player");
            }
        }
        Ok(())
    }

    pub fn cancel_pending(&mut self) {
        self.pending_gate = None;
        self.pending_density = None;
        self.pending_transposition = None;
        self.pending_selection_offset = None;
    }

    pub fn begin_step(
        &mut self,
        at: Beat,
        index: i64,
        revision: u64,
        capture_ready: bool,
        cycle: bool,
    ) -> bool {
        if let Some(value) = self.pending_gate.take() {
            self.gate = value;
        }
        if let Some(value) = self.pending_density.take() {
            self.density = value;
        }
        if let Some(value) = self.pending_transposition.take() {
            self.transposition = value;
        }
        if let Some(value) = self.pending_selection_offset.take() {
            self.selection_offset = value;
        }
        if self.events.len() > 4094 - usize::from(capture_ready) - usize::from(cycle) {
            self.exhausted = true;
            return false;
        }
        if capture_ready {
            self.events.push(PortEvent {
                at,
                port: OutputPort::CaptureReady,
                index,
                revision,
            });
        }
        if cycle {
            self.events.push(PortEvent {
                at,
                port: OutputPort::Cycle,
                index,
                revision,
            });
        }
        self.events.push(PortEvent {
            at,
            port: OutputPort::Step,
            index,
            revision,
        });
        true
    }

    pub fn offset_rank(&self, rank: usize, size: usize) -> Option<usize> {
        let shifted = rank as i128 + i128::from(self.selection_offset);
        if self.offset_rest_outside && !(0..size as i128).contains(&shifted) {
            return None;
        }
        Some(shifted.rem_euclid(size as i128) as usize)
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

    pub fn outcome(&mut self, hit: bool) {
        let step = *self.events.last().expect("step precedes its outcome");
        self.events.push(PortEvent {
            port: if hit {
                OutputPort::Hit
            } else {
                OutputPort::Rest
            },
            ..step
        });
    }

    pub fn take_events(&mut self) -> PortBatch {
        PortBatch {
            events: std::mem::take(&mut self.events),
            notes: Vec::new(),
            exhausted: std::mem::take(&mut self.exhausted),
        }
    }
}

pub const REGISTER_RANGE_ERROR: &str = "pitch is outside configured register limits";
