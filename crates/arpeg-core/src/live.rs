//! Incremental decisions for a running, held-note arpeggiator.

use crate::{Beat, Selection};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputKind {
    NoteOn { id: u64, key: u8, velocity: u8 },
    NoteOff { id: u64, key: u8 },
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
    key: u8,
    end: Beat,
}

pub struct LiveArpeggiator {
    selection: Selection,
    step: Beat,
    gate: Beat,
    next_step: Beat,
    now: Beat,
    next_id: u64,
    next_output: u64,
    previous_key: Option<(Beat, u64)>,
    input: Vec<InputNote>,
    sounding: Vec<SoundingNote>,
}

impl LiveArpeggiator {
    pub fn new(selection: Selection, step: Beat, gate: Beat) -> Result<Self, &'static str> {
        if step <= Beat::from_integer(0) || gate < Beat::from_integer(0) {
            return Err("step must be positive and gate nonnegative");
        }
        Ok(Self {
            selection,
            step,
            gate,
            next_step: Beat::from_integer(0),
            now: Beat::from_integer(0),
            next_id: 0,
            next_output: 0,
            previous_key: None,
            input: Vec::new(),
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
        let output = self.process_until(at, false);
        self.input.push(InputNote {
            id: self.next_id,
            key,
            velocity,
            onset: at,
        });
        self.next_id += 1;
        Ok(output)
    }

    pub fn note_off(&mut self, at: Beat, key: u8) -> Result<Vec<OutputEvent>, &'static str> {
        self.check_time(at)?;
        let mut output = self.process_until(at, false);
        let index = self
            .input
            .iter()
            .position(|note| note.key == key)
            .ok_or("release has no matching onset")?;
        self.input.remove(index);
        if self.input.is_empty() {
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
        self.previous_key = None;
        self.now = at;
        Ok(output)
    }

    pub fn next_deadline(&self) -> Beat {
        self.sounding
            .iter()
            .map(|note| note.end)
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
            let release = self.sounding.iter().map(|note| note.end).min();
            let next = release.map_or(self.next_step, |end| end.min(self.next_step));
            if next > through || (!inclusive && next == through) {
                break;
            }
            self.release_due(next, &mut output);
            if self.next_step == next {
                self.play_step(next, &mut output);
                self.next_step += self.step;
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
        self.sounding
            .drain(..)
            .map(|note| OutputEvent {
                at,
                kind: OutputKind::NoteOff {
                    id: note.id,
                    key: note.key,
                },
            })
            .collect()
    }

    fn play_step(&mut self, at: Beat, output: &mut Vec<OutputEvent>) {
        if self.input.is_empty() {
            self.previous_key = None;
            return;
        }
        let selection = self.selection;
        self.input.sort_unstable_by_key(|note| match selection {
            Selection::Ascending => (Beat::from_integer(i64::from(note.key)), note.id),
            Selection::Descending => (-Beat::from_integer(i64::from(note.key)), note.id),
            Selection::Played => (note.onset, note.id),
            Selection::ReversePlayed => (-note.onset, note.id),
        });
        let selected = self
            .input
            .iter()
            .find(|note| {
                self.previous_key.is_none_or(|previous| {
                    let key = match selection {
                        Selection::Ascending => Beat::from_integer(i64::from(note.key)),
                        Selection::Descending => -Beat::from_integer(i64::from(note.key)),
                        Selection::Played => note.onset,
                        Selection::ReversePlayed => -note.onset,
                    };
                    (key, note.id) > previous
                })
            })
            .unwrap_or(&self.input[0]);
        let key = match selection {
            Selection::Ascending => Beat::from_integer(i64::from(selected.key)),
            Selection::Descending => -Beat::from_integer(i64::from(selected.key)),
            Selection::Played => selected.onset,
            Selection::ReversePlayed => -selected.onset,
        };
        self.previous_key = Some((key, selected.id));
        let end = at + self.step * self.gate;
        for note in self
            .sounding
            .iter_mut()
            .filter(|note| note.key == selected.key)
        {
            output.push(OutputEvent {
                at,
                kind: OutputKind::NoteOff {
                    id: note.id,
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
                key: selected.key,
                velocity: selected.velocity,
            },
        });
        if end == at {
            output.push(OutputEvent {
                at,
                kind: OutputKind::NoteOff {
                    id,
                    key: selected.key,
                },
            });
        } else {
            self.sounding.push(SoundingNote {
                id,
                key: selected.key,
                end,
            });
        }
    }
}
