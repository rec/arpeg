//! Payload-independent timing controls and decision notifications.
use crate::Beat;

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
    pub channel: u8,
    pub at: Beat,
    pub port: OutputPort,
    pub index: i64,
    pub revision: u64,
}

pub struct DecisionBatch {
    pub events: Vec<PortEvent>,
    pub exhausted: bool,
}

pub struct TimingPorts {
    pub gate: Beat,
    pub density: Beat,
    pub selection_offset: i64,
    pub offset_rest_outside: bool,
    pending_gate: Option<Beat>,
    pending_density: Option<Beat>,
    pending_selection_offset: Option<i64>,
    events: Vec<PortEvent>,
    exhausted: bool,
}

impl TimingPorts {
    pub fn new(gate: Beat, density: Beat) -> Self {
        Self {
            gate,
            density,
            selection_offset: 0,
            offset_rest_outside: false,
            pending_gate: None,
            pending_density: None,
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
            InputPort::Breath | InputPort::Bend | InputPort::Pressure | InputPort::Transposition
        ) {
            return Err("control is not supported by the item engine");
        }
        if matches!(port, InputPort::SelectionOffset) {
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
            InputPort::SelectionOffset => self.pending_selection_offset = Some(value.to_integer()),
            InputPort::Breath
            | InputPort::Bend
            | InputPort::Pressure
            | InputPort::Transposition => {
                return Err("control is not supported by the item engine");
            }
        }
        Ok(())
    }

    pub fn cancel_pending(&mut self) {
        self.pending_gate = None;
        self.pending_density = None;
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
        if let Some(value) = self.pending_selection_offset.take() {
            self.selection_offset = value;
        }
        if self.events.len() > 4094 - usize::from(capture_ready) - usize::from(cycle) {
            self.exhausted = true;
            return false;
        }
        if capture_ready {
            self.events.push(PortEvent {
                channel: 1,
                at,
                port: OutputPort::CaptureReady,
                index,
                revision,
            });
        }
        if cycle {
            self.events.push(PortEvent {
                channel: 1,
                at,
                port: OutputPort::Cycle,
                index,
                revision,
            });
        }
        self.events.push(PortEvent {
            channel: 1,
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

    pub fn take_events(&mut self) -> DecisionBatch {
        DecisionBatch {
            events: std::mem::take(&mut self.events),
            exhausted: std::mem::take(&mut self.exhausted),
        }
    }
}
