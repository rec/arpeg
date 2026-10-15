//! Opaque token handles for hosts that own text or other payloads.

use crate::engine::{Occurrence, Processor};
use crate::timing_ports::TimingPorts;

pub struct TokenPorts {
    pub timing: TimingPorts,
}

impl Processor for TokenPorts {
    type Value = u64;
    type Event = Occurrence<u64>;

    fn timing(&self) -> &TimingPorts {
        &self.timing
    }

    fn timing_mut(&mut self) -> &mut TimingPorts {
        &mut self.timing
    }

    fn event(&self, occurrence: Occurrence<u64>) -> Occurrence<u64> {
        occurrence
    }
}
