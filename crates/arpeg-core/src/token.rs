//! Opaque token handles for hosts that own text or other payloads.

use crate::Beat;
use crate::engine::Processor;
use crate::timing_ports::TimingPorts;

pub struct TokenPorts {
    pub timing: TimingPorts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokenEvent {
    pub at: Beat,
    pub start: bool,
    pub id: u64,
    pub source_id: u64,
    pub value: u64,
}

impl Processor for TokenPorts {
    type Value = u64;
    type Event = TokenEvent;

    fn timing(&self) -> &TimingPorts {
        &self.timing
    }

    fn timing_mut(&mut self) -> &mut TimingPorts {
        &mut self.timing
    }

    fn event(&self, at: Beat, start: bool, id: u64, source_id: u64, value: u64) -> TokenEvent {
        TokenEvent {
            at,
            start,
            id,
            source_id,
            value,
        }
    }
}
