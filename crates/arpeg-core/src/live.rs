//! Incremental decisions for a running, held-note arpeggiator.

use crate::{
    Bank, Beat, Selection,
    rhythm::{Rhythm, RhythmDecision},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Retrigger {
    OnEmpty,
    BankEdit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputKind {
    NoteOn {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OutputEvent {
    pub at: Beat,
    pub kind: OutputKind,
}

#[derive(Clone, Copy, Debug)]
struct InputNote {
    id: u64,
    key: u8,
    velocity: u8,
    onset: Beat,
}

#[derive(Clone, Copy, Debug)]
struct SoundingNote {
    id: u64,
    source_id: u64,
    key: u8,
    end: Beat,
}

#[derive(Clone, Copy, Debug)]
struct Attack {
    at: Beat,
    note: InputNote,
    gate: Beat,
}

pub struct LiveArpeggiator {
    bank_mode: Bank,
    retrigger: Retrigger,
    selection: Selection,
    rhythm: Rhythm,
    gate: Beat,
    next_step: Beat,
    step_index: i64,
    pending: Vec<Attack>,
    now: Beat,
    next_id: u64,
    next_output: u64,
    previous_key: Option<(Beat, u64)>,
    input: Vec<InputNote>,
    bank: Vec<InputNote>,
    toggle_at: Option<Beat>,
    toggled_keys: Vec<u8>,
    toggle_added_keys: Vec<u8>,
    sounding: Vec<SoundingNote>,
}

impl LiveArpeggiator {
    pub fn new(
        bank: Bank,
        selection: Selection,
        rhythm: Rhythm,
        gate: Beat,
        retrigger: Retrigger,
    ) -> Result<Self, &'static str> {
        rhythm.validate()?;
        if gate < Beat::from_integer(0) {
            return Err("gate must be nonnegative");
        }
        Ok(Self {
            bank_mode: bank,
            retrigger,
            selection,
            rhythm,
            gate,
            next_step: Beat::from_integer(0),
            step_index: 0,
            pending: Vec::new(),
            now: Beat::from_integer(0),
            next_id: 0,
            next_output: 0,
            previous_key: None,
            input: Vec::new(),
            bank: Vec::new(),
            toggle_at: None,
            toggled_keys: Vec::new(),
            toggle_added_keys: Vec::new(),
            sounding: Vec::new(),
        })
    }

    pub fn note_on(
        &mut self,
        at: Beat,
        key: u8,
        velocity: u8,
    ) -> Result<Vec<OutputEvent>, &'static str> {
        self.check_time(at)?;
        let mut output = self.process_until(at, false);
        let new_chord = self.input.is_empty();
        let note = InputNote {
            id: self.next_id,
            key,
            velocity,
            onset: at,
        };
        self.input.push(note);
        let edited = match self.bank_mode {
            Bank::Held => true,
            Bank::LatchedReplace => {
                if new_chord {
                    self.bank.clear();
                }
                self.bank.push(note);
                true
            }
            Bank::LatchedAdd => {
                self.bank.push(note);
                true
            }
            Bank::LatchedToggle => {
                if self.toggle_at != Some(at) {
                    self.toggle_at = Some(at);
                    self.toggled_keys.clear();
                    self.toggle_added_keys.clear();
                }
                if !self.toggled_keys.contains(&key) {
                    self.toggled_keys.push(key);
                    if self.bank.iter().any(|entry| entry.key == key) {
                        self.bank.retain(|entry| entry.key != key);
                    } else {
                        self.bank.push(note);
                        self.toggle_added_keys.push(key);
                    }
                    true
                } else if self.toggle_added_keys.contains(&key) {
                    self.bank.push(note);
                    true
                } else {
                    false
                }
            }
        };
        if self.bank_mode != Bank::Held && self.bank.is_empty() {
            self.previous_key = None;
            output.extend(self.release_all(at));
        }
        if edited && self.retrigger == Retrigger::BankEdit {
            self.previous_key = None;
        }
        self.next_id += 1;
        Ok(output)
    }

    pub fn note_off(&mut self, at: Beat, key: u8) -> Result<Vec<OutputEvent>, &'static str> {
        self.check_time(at)?;
        let index = self
            .input
            .iter()
            .position(|note| note.key == key)
            .ok_or("release has no matching onset")?;
        let mut output = self.process_until(at, false);
        self.input.remove(index);
        if self.bank_mode == Bank::Held && self.retrigger == Retrigger::BankEdit {
            self.previous_key = None;
        }
        if self.bank_mode == Bank::Held && self.input.is_empty() {
            self.previous_key = None;
            output.extend(self.release_all(at));
        }
        Ok(output)
    }

    pub fn advance(&mut self, through: Beat) -> Result<Vec<OutputEvent>, &'static str> {
        self.check_time(through)?;
        Ok(self.process_until(through, true))
    }

    pub fn stop(&mut self, at: Beat) -> Result<Vec<OutputEvent>, &'static str> {
        self.check_time(at)?;
        let mut output = self.process_until(at, false);
        output.extend(self.release_all(at));
        self.input.clear();
        self.bank.clear();
        self.toggle_at = None;
        self.toggled_keys.clear();
        self.toggle_added_keys.clear();
        self.previous_key = None;
        self.now = at;
        Ok(output)
    }

    pub fn clear(&mut self, at: Beat) -> Result<Vec<OutputEvent>, &'static str> {
        self.check_time(at)?;
        if self.bank_mode == Bank::Held {
            return Err("clear requires a latched bank");
        }
        let mut output = self.process_until(at, false);
        self.bank.clear();
        self.toggle_at = None;
        self.toggled_keys.clear();
        self.toggle_added_keys.clear();
        self.previous_key = None;
        output.extend(self.release_all(at));
        Ok(output)
    }

    pub fn next_deadline(&self) -> Beat {
        self.sounding
            .iter()
            .map(|note| note.end)
            .chain(self.pending.iter().map(|attack| attack.at))
            .min()
            .map_or(self.next_step, |end| end.min(self.next_step))
    }

    fn check_time(&self, at: Beat) -> Result<(), &'static str> {
        if at < self.now {
            Err("live time must not go backwards")
        } else {
            Ok(())
        }
    }

    fn process_until(&mut self, through: Beat, inclusive: bool) -> Vec<OutputEvent> {
        let mut output = Vec::new();
        loop {
            let next = self.next_deadline();
            if next > through || (!inclusive && next == through) {
                break;
            }
            self.release_due(next, &mut output);
            if self.next_step == next {
                let decision = self.rhythm.decide_step(self.step_index, self.gate);
                self.schedule_step(next, decision);
                self.next_step += decision.duration;
                self.step_index += 1;
            }
            while self.pending.first().is_some_and(|attack| attack.at == next) {
                let attack = self.pending.remove(0);
                self.attack(attack, &mut output);
            }
        }
        self.now = through;
        output
    }

    fn release_due(&mut self, at: Beat, output: &mut Vec<OutputEvent>) {
        self.sounding.retain(|note| {
            if note.end <= at {
                output.push(OutputEvent {
                    at: note.end,
                    kind: OutputKind::NoteOff {
                        id: note.id,
                        source_id: note.source_id,
                        key: note.key,
                    },
                });
                false
            } else {
                true
            }
        });
    }

    fn release_all(&mut self, at: Beat) -> Vec<OutputEvent> {
        self.pending.clear();
        self.sounding
            .drain(..)
            .map(|note| OutputEvent {
                at,
                kind: OutputKind::NoteOff {
                    id: note.id,
                    source_id: note.source_id,
                    key: note.key,
                },
            })
            .collect()
    }

    fn schedule_step(&mut self, at: Beat, decision: RhythmDecision) {
        let active = if self.bank_mode == Bank::Held {
            &self.input
        } else {
            &self.bank
        };
        if active.is_empty() {
            self.previous_key = None;
            return;
        }
        if decision.repeats == 0 {
            return;
        }
        let selection = self.selection;
        let mut ordered = active.clone();
        ordered.sort_unstable_by_key(|note| selection_key(note, selection));
        let selected = *ordered
            .iter()
            .find(|note| {
                self.previous_key
                    .is_none_or(|previous| selection_key(note, selection) > previous)
            })
            .unwrap_or(&ordered[0]);
        self.previous_key = Some(selection_key(&selected, selection));
        let interval = decision.duration / decision.repeats as i64;
        self.pending.extend((0..decision.repeats).map(|i| Attack {
            at: at + interval * i as i64,
            note: selected,
            gate: if i == decision.repeats - 1 {
                decision.final_gate
            } else {
                decision.gate
            },
        }));
    }

    fn attack(&mut self, attack: Attack, output: &mut Vec<OutputEvent>) {
        let at = attack.at;
        let selected = attack.note;
        let end = at + attack.gate;
        for note in self
            .sounding
            .iter_mut()
            .filter(|note| note.key == selected.key)
        {
            output.push(OutputEvent {
                at,
                kind: OutputKind::NoteOff {
                    id: note.id,
                    source_id: note.source_id,
                    key: note.key,
                },
            });
            note.end = at;
        }
        self.sounding.retain(|note| note.end > at);
        let id = self.next_output;
        self.next_output += 1;
        output.push(OutputEvent {
            at,
            kind: OutputKind::NoteOn {
                id,
                source_id: selected.id,
                key: selected.key,
                velocity: selected.velocity,
            },
        });
        if end == at {
            output.push(OutputEvent {
                at,
                kind: OutputKind::NoteOff {
                    id,
                    source_id: selected.id,
                    key: selected.key,
                },
            });
        } else {
            self.sounding.push(SoundingNote {
                id,
                source_id: selected.id,
                key: selected.key,
                end,
            });
        }
    }
}

fn selection_key(note: &InputNote, selection: Selection) -> (Beat, u64) {
    let position = match selection {
        Selection::Ascending => Beat::from_integer(i64::from(note.key)),
        Selection::Descending => -Beat::from_integer(i64::from(note.key)),
        Selection::Played => note.onset,
        Selection::ReversePlayed => -note.onset,
    };
    (position, note.id)
}
