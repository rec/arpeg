//! Host-clock connections between arpeg and enveLFO, without device or audio IO.
//!
//! Deliver hits at their host timestamps, then observe once and apply expression
//! after arpeg's note messages at that same timestamp. The host owns cadence,
//! transport mapping, lane ownership and delivery. Neither core depends on the
//! other. Cloning a bridge preserves pending Patch deliveries and random state.

use std::collections::BTreeMap;

use arpeg_core::{
    Beat,
    ports::{InputPort, OutputPort, PortEvent},
};
use envelfo_core::{
    Error,
    patch::{ChildCommand, ChildEvent, ClockMapping, Node, Patch, PatchState},
    stages::OutputEvent,
    time::Time,
};

#[derive(Clone, Debug, PartialEq)]
pub struct ExpressionObservation {
    pub values: BTreeMap<String, f64>,
    pub controls: Vec<(InputPort, Beat)>,
    pub events: Vec<OutputEvent>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EnveLFOBridge {
    definition: Patch,
    pub state: PatchState,
    hit_target: String,
    expression: Vec<(InputPort, String)>,
}

impl EnveLFOBridge {
    pub fn new(
        definition: Patch,
        state: PatchState,
        hit_target: String,
        expression: Vec<(InputPort, String)>,
    ) -> Result<Self, Error> {
        if !definition
            .children
            .iter()
            .any(|c| c.name == hit_target && matches!(c.node, Node::Envelope(_)))
        {
            return Err(Error::InvalidDefinition(
                "hit target must be a Patch-owned Envelope",
            ));
        }
        for (i, (port, name)) in expression.iter().enumerate() {
            if !matches!(
                port,
                InputPort::Breath | InputPort::Bend | InputPort::Pressure
            ) || expression[..i].iter().any(|(p, _)| p == port)
                || !definition.outputs.iter().any(|o| o.name == *name)
            {
                return Err(Error::InvalidDefinition(
                    "expression mappings require unique expression lanes and named Patch outputs",
                ));
            }
        }
        Ok(Self {
            definition,
            state,
            hit_target,
            expression,
        })
    }

    /// Translate a hit using explicit host seconds, not its source beat position.
    pub fn receive(
        &mut self,
        at: Time,
        event: &PortEvent,
        clock: &impl ClockMapping,
    ) -> Result<Vec<OutputEvent>, Error> {
        if event.port != OutputPort::Hit {
            return Ok(vec![]);
        }
        let ordinal = self
            .definition
            .next_ordinal(&self.state, &self.hit_target)?;
        self.definition.command(
            &mut self.state,
            &self.hit_target,
            &ChildEvent {
                at,
                ordinal,
                command: ChildCommand::Start,
            },
            clock,
            false,
        )
    }

    /// Observe after all same-time hits. Invalid expression leaves state intact.
    pub fn observe(
        &mut self,
        at: Time,
        next_delivery: Time,
        clock: &impl ClockMapping,
    ) -> Result<ExpressionObservation, Error> {
        let mut state = self.state.clone();
        let result = self
            .definition
            .advance(&mut state, at, next_delivery, clock)?;
        let mut controls = Vec::with_capacity(self.expression.len());
        for (port, name) in &self.expression {
            let value = result.values[name];
            let lower = if *port == InputPort::Bend { -1.0 } else { 0.0 };
            if !value.is_finite() || !(lower..=1.0).contains(&value) {
                return Err(Error::InvalidDefinition(
                    "expression sample is outside its normalized range",
                ));
            }
            let value = Beat::approximate_float(value).ok_or(Error::Overflow)?;
            controls.push((*port, value));
        }
        self.state = state;
        Ok(ExpressionObservation {
            values: result.values,
            controls,
            events: result.events,
        })
    }
}
