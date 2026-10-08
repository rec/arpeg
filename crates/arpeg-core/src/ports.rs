//! Step-published scalar inputs and bounded discrete Motion outputs.

use crate::Beat;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputPort {
    Gate,
    Density,
    Transposition,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PitchBoundary {
    #[default]
    Drop,
    Fold,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputPort {
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
    pub exhausted: bool,
}

pub struct PerformancePorts {
    pub gate: Beat,
    pub density: Beat,
    pub transposition: i64,
    pub pitch_boundary: PitchBoundary,
    pending_gate: Option<Beat>,
    pending_density: Option<Beat>,
    pending_transposition: Option<i64>,
    events: Vec<PortEvent>,
    exhausted: bool,
}

impl PerformancePorts {
    pub fn new(gate: Beat, density: Beat) -> Self {
        Self {
            gate,
            density,
            transposition: 0,
            pitch_boundary: PitchBoundary::Drop,
            pending_gate: None,
            pending_density: None,
            pending_transposition: None,
            events: Vec::new(),
            exhausted: false,
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
                Err("transposition requires whole semitones")
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

    pub fn queue(&mut self, port: InputPort, value: Beat) {
        match port {
            InputPort::Gate => self.pending_gate = Some(value),
            InputPort::Density => self.pending_density = Some(value),
            InputPort::Transposition => self.pending_transposition = Some(value.to_integer()),
        }
    }

    pub fn cancel_pending(&mut self) {
        self.pending_gate = None;
        self.pending_density = None;
        self.pending_transposition = None;
    }

    pub fn begin_step(&mut self, at: Beat, index: i64, revision: u64) -> bool {
        if let Some(value) = self.pending_gate.take() {
            self.gate = value;
        }
        if let Some(value) = self.pending_density.take() {
            self.density = value;
        }
        if let Some(value) = self.pending_transposition.take() {
            self.transposition = value;
        }
        if self.events.len() > 4094 {
            self.exhausted = true;
            return false;
        }
        self.events.push(PortEvent {
            at,
            port: OutputPort::Step,
            index,
            revision,
        });
        true
    }

    pub fn realize_pitch(&self, key: u8) -> Result<Option<u8>, &'static str> {
        let pitch = i128::from(key) + i128::from(self.transposition);
        if (0..=127).contains(&pitch) {
            return Ok(Some(pitch as u8));
        }
        match self.pitch_boundary {
            PitchBoundary::Drop => Ok(None),
            PitchBoundary::Fold => Ok(Some(if pitch < 0 {
                pitch.rem_euclid(12) as u8
            } else {
                (116 + (pitch - 116).rem_euclid(12)) as u8
            })),
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
            exhausted: std::mem::take(&mut self.exhausted),
        }
    }
}
