//! Musical transforms and MIDI realization around the shared item engine.
pub use crate::engine::Retrigger;
use crate::engine::{Arpeggiator, Item, Occurrence, Processor, SelectionPosition};
use crate::inversion::inversion_offsets;
use crate::octaves::Traversal;
use crate::ports::{InputPort, PerformancePorts};
use crate::timing_ports::TimingPorts;
use crate::{Bank, Beat, Selection, chance::Chance, rhythm::Rhythm};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutputKind {
    NoteOn {
        tuning_semitones: f64,
        id: u64,
        source_id: u64,
        key: u8,
        velocity: u8,
    },
    NoteOff {
        id: u64,
        source_id: u64,
        key: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputEvent {
    pub at: Beat,
    pub kind: OutputKind,
}

#[derive(Clone, Copy, Debug)]
pub struct MusicalValue {
    pub key: u8,
    pub velocity: u8,
    pub tuning_semitones: f64,
}

pub type LiveArpeggiator = Arpeggiator<PerformancePorts>;

impl Arpeggiator<PerformancePorts> {
    pub fn musical(
        bank: Bank,
        selection: Selection,
        rhythm: Rhythm,
        gate: Beat,
        retrigger: Retrigger,
        chance: Chance,
    ) -> Result<Self, &'static str> {
        let ports = PerformancePorts::new(gate, chance.probability);
        Self::new(bank, selection, rhythm, ports, retrigger, chance)
    }
    pub fn note_on(
        &mut self,
        at: Beat,
        key: u8,
        velocity: u8,
    ) -> Result<Vec<OutputEvent>, &'static str> {
        if key > 127 || velocity == 0 || velocity > 127 {
            return Err("note key and velocity must be MIDI values");
        }
        self.press(
            at,
            u64::from(key),
            i128::from(key),
            MusicalValue {
                key,
                velocity,
                tuning_semitones: 0.0,
            },
        )
    }
    pub fn note_off(&mut self, at: Beat, key: u8) -> Result<Vec<OutputEvent>, &'static str> {
        self.release(at, u64::from(key))
    }
}

impl Processor for PerformancePorts {
    type Value = MusicalValue;
    type Event = OutputEvent;
    fn timing(&self) -> &TimingPorts {
        &self.timing
    }
    fn timing_mut(&mut self) -> &mut TimingPorts {
        &mut self.timing
    }
    fn expand(
        &self,
        active: &[Item<MusicalValue>],
    ) -> Result<Vec<Item<MusicalValue>>, &'static str> {
        if self.octaves.offsets.is_empty() {
            return Err("octave offsets must not be empty");
        }
        let period = self.tuning.as_ref().and_then(|t| t.period).unwrap_or(12) as i128;
        let offsets = inversion_offsets(
            &active.iter().map(|n| n.value.key).collect::<Vec<_>>(),
            self.inversion,
            period,
        );
        let mut voiced = vec![0; active.len()];
        if !self.voicing.is_empty() {
            let mut ranked: Vec<_> = (0..active.len()).collect();
            ranked.sort_unstable_by_key(|i| {
                (
                    i128::from(active[*i].value.key) + offsets[*i],
                    active[*i].id,
                )
            });
            for (i, shift) in ranked.into_iter().zip(&self.voicing) {
                voiced[i] = period * i128::from(*shift);
            }
        }
        let active: Vec<_> = self
            .octaves
            .indices(active.len(), false)
            .into_iter()
            .map(|(i, j)| Item {
                id: active[i].id * self.octaves.offsets.len() as u64 + j as u64,
                source_id: active[i].id,
                layer: j,
                rank: i128::from(active[i].value.key) + offsets[i] + voiced[i],
                ..active[i]
            })
            .collect();
        Ok(active)
    }
    fn realize(
        &self,
        selected: &Item<MusicalValue>,
        accent: Beat,
    ) -> Result<Option<MusicalValue>, &'static str> {
        let period = self.tuning.as_ref().and_then(|t| t.period).unwrap_or(12) as i128;
        let pitch = selected
            .rank
            .checked_add(period * i128::from(self.octaves.offsets[selected.layer]))
            .ok_or(crate::tuning::TUNING_ERROR)?;
        Ok(self.realize_pitch(pitch)?.map(|key| MusicalValue {
            key: key.key,
            velocity: accent_velocity(selected.value.velocity, accent),
            tuning_semitones: key.tuning_semitones,
        }))
    }
    fn conflicts(&self, left: MusicalValue, right: MusicalValue) -> bool {
        left.key == right.key
    }
    fn event(&self, occurrence: Occurrence<MusicalValue>) -> OutputEvent {
        let Occurrence {
            at,
            start,
            id,
            source_id,
            value,
            ..
        } = occurrence;
        OutputEvent {
            at,
            kind: if start {
                OutputKind::NoteOn {
                    tuning_semitones: value.tuning_semitones,
                    id,
                    source_id,
                    key: value.key,
                    velocity: value.velocity,
                }
            } else {
                OutputKind::NoteOff {
                    id,
                    source_id,
                    key: value.key,
                }
            },
        }
    }
    fn entry_key(
        &self,
        item: &Item<MusicalValue>,
        position: SelectionPosition,
        reverse: bool,
    ) -> (i64, SelectionPosition, u64, i64, u64) {
        let layer = item.layer as i64 * if reverse { -1 } else { 1 };
        match self.octaves.traversal {
            Traversal::NotesFirst => (layer, position, item.source_id, 0, item.id),
            Traversal::OctavesFirst => (0, position, item.source_id, layer, item.id),
        }
    }
    fn check_control(
        &self,
        port: InputPort,
        value: Beat,
        seed: Option<i64>,
    ) -> Result<(), &'static str> {
        Self::check_control(port, value, seed)
    }
    fn queue(&mut self, port: InputPort, value: Beat) -> Result<(), &'static str> {
        Self::queue(self, port, value)
    }
    fn cancel_pending(&mut self) {
        Self::cancel_pending(self);
    }
    fn begin_step(&mut self, at: Beat, index: i64, revision: u64, cycle: bool) -> bool {
        Self::begin_step(self, at, index, revision, false, cycle)
    }
}

/// Round a positive multiplier exactly, preserving a nonzero attack encoding.
pub fn accent_velocity(velocity: u8, accent: Beat) -> u8 {
    let numerator = i128::from(velocity) * i128::from(*accent.numer());
    let denominator = i128::from(*accent.denom());
    ((2 * numerator + denominator) / (2 * denominator)).clamp(1, 127) as u8
}
