//! Step-published history and explicitly committed phrase takes.

use crate::chance::Chance;
use crate::octaves::Octaves;
use crate::{
    Selection,
    capture::{CapturedPhrase, MidiCapture, MidiEvent, Profile, Timebase},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureMode {
    History(usize),
    Phrase,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BankNote {
    pub capture_id: String,
    pub note_id: String,
    pub octave_index: usize,
}

pub struct CaptureBank {
    pub octaves: Octaves,
    pub mode: CaptureMode,
    pub recording: Option<MidiCapture>,
    pub published: Vec<BankNote>,
    pub revision: usize,
    pub last_selected: Option<BankNote>,
    selection: Selection,
    retrigger: bool,
    selected_revision: usize,
    live_snapshot: Option<CapturedPhrase>,
    takes: Vec<Take>,
    history_floor: usize,
    pub pattern_position: usize,
    choice_count: u64,
}

impl CaptureBank {
    pub fn new(
        mode: CaptureMode,
        selection: Selection,
        retrigger: bool,
        profile: Profile,
    ) -> Result<Self, &'static str> {
        if mode == CaptureMode::History(0) {
            return Err("history requires positive capacity");
        }
        if !matches!(
            selection,
            Selection::Ascending
                | Selection::Descending
                | Selection::Played
                | Selection::ReversePlayed
                | Selection::IndexPattern { .. }
                | Selection::Transition(_)
                | Selection::IntervalEnvelope(_)
        ) {
            return Err("unsupported captured note selection");
        }
        if let Selection::IndexPattern {
            indices, intervals, ..
        } = &selection
        {
            if indices.is_empty() {
                return Err("index pattern requires at least one index");
            }
            if intervals.as_ref().is_some_and(|v| v.len() != indices.len()) {
                return Err("intervals must have one value per index");
            }
        }
        if let Selection::Transition(transition) = &selection {
            if transition.weights.is_empty() {
                return Err("transition requires a nonempty weight table");
            }
        }
        if let Selection::IntervalEnvelope(envelope) = &selection {
            envelope.validate()?;
        }
        let mut bank = Self {
            octaves: Octaves::default(),
            mode,
            recording: None,
            published: Vec::new(),
            revision: 0,
            last_selected: None,
            selection,
            retrigger,
            selected_revision: 0,
            live_snapshot: None,
            takes: Vec::new(),
            history_floor: 0,
            pattern_position: 0,
            choice_count: 0,
        };
        if matches!(mode, CaptureMode::History(_)) {
            bank.record("live", profile)?;
        }
        Ok(bank)
    }

    pub fn check_control(&self, command: &str) -> Result<(), &'static str> {
        match command {
            "record" if self.recording.is_some() => Err("a capture is already recording"),
            "commit" | "overdub" if self.recording.is_none() => Err("record before committing"),
            "commit" | "overdub" if self.mode == CaptureMode::Phrase && self.takes.len() >= 128 => {
                Err("phrase reached its 128-take limit; undo or clear first")
            }
            "undo" if self.takes.is_empty() => Err("there is no committed capture to undo"),
            "record" | "commit" | "overdub" | "undo" => Ok(()),
            _ => Err("capture command must be record, commit, overdub or undo"),
        }
    }

    pub fn record(&mut self, id: &str, profile: Profile) -> Result<(), &'static str> {
        self.check_control("record")?;
        if self.takes.iter().any(|t| t.phrase.capture_id == id) {
            return Err("capture ID already exists");
        }
        self.recording = Some(MidiCapture::new(
            id,
            Timebase {
                name: "microseconds".into(),
                rate_numerator: 1_000_000,
                rate_denominator: 1,
            },
            profile,
        )?);
        self.live_snapshot = None;
        self.history_floor = 0;
        Ok(())
    }

    pub fn accept(&mut self, event: MidiEvent) -> Result<(), &'static str> {
        self.recording
            .as_mut()
            .ok_or("record before accepting MIDI")?
            .accept(event, None)
    }

    pub fn commit(&mut self, tick: i64, overdub: bool) -> Result<(), &'static str> {
        self.check_control("commit")?;
        self.recording
            .as_ref()
            .expect("validated recording")
            .check_end(tick)?;
        let phrase = self
            .recording
            .take()
            .expect("validated recording")
            .finish(tick)?;
        self.takes.push(Take { phrase, overdub });
        self.live_snapshot = None;
        Ok(())
    }

    pub fn undo(&mut self) -> Result<(), &'static str> {
        self.check_control("undo")?;
        self.takes.pop();
        Ok(())
    }

    pub fn clear(&mut self) {
        self.recording = None;
        self.live_snapshot = None;
        self.takes.clear();
        if !self.published.is_empty() {
            self.revision += 1;
        }
        self.published.clear();
        self.last_selected = None;
        self.pattern_position = 0;
    }

    pub fn clear_history(&mut self) -> Result<(), &'static str> {
        if !matches!(self.mode, CaptureMode::History(_)) {
            return Err("clear_history requires an active history capture");
        }
        self.history_floor = self
            .recording
            .as_ref()
            .ok_or("clear_history requires an active history capture")?
            .note_count();
        self.takes.clear();
        self.published.clear();
        self.last_selected = None;
        self.pattern_position = 0;
        self.revision += 1;
        Ok(())
    }

    pub fn advance(&mut self, tick: i64) -> Result<(), &'static str> {
        if matches!(self.mode, CaptureMode::History(_)) {
            if let Some(capture) = &mut self.recording {
                self.live_snapshot = Some(capture.snapshot(tick)?);
            }
        }
        Ok(())
    }

    pub fn publish_step(&mut self) {
        let mut notes = Vec::new();
        for take in &self.takes {
            if !take.overdub {
                notes.clear();
            }
            notes.extend(take.phrase.notes.iter().map(|n| BankNote {
                capture_id: take.phrase.capture_id.clone(),
                note_id: n.note_id.clone(),
                octave_index: 0,
            }));
        }
        if let CaptureMode::History(capacity) = self.mode {
            if let Some(phrase) = &self.live_snapshot {
                notes.extend(
                    phrase
                        .notes
                        .iter()
                        .skip(self.history_floor)
                        .map(|n| BankNote {
                            capture_id: phrase.capture_id.clone(),
                            note_id: n.note_id.clone(),
                            octave_index: 0,
                        }),
                );
            }
            if notes.len() > capacity {
                notes.drain(..notes.len() - capacity);
            }
        }
        if notes != self.published {
            self.published = notes;
            self.revision += 1;
        }
    }

    pub fn select_step(
        &mut self,
        chance: &Chance,
        step: i64,
    ) -> Result<Option<BankNote>, &'static str> {
        self.publish_step();
        if self.published.is_empty() {
            self.last_selected = None;
            self.pattern_position = 0;
            return Ok(None);
        }
        if self.selected_revision != self.revision {
            if self.retrigger {
                self.last_selected = None;
                self.pattern_position = 0;
            }
            self.selected_revision = self.revision;
        }
        let ordered = self.ordered_notes(match self.selection {
            Selection::Transition(_)
            | Selection::IntervalEnvelope(_)
            | Selection::IndexPattern { .. } => &Selection::Ascending,
            _ => &self.selection,
        })?;
        let position = self
            .last_selected
            .as_ref()
            .and_then(|r| ordered.iter().position(|n| n == r));
        let rank = match &self.selection {
            Selection::IndexPattern {
                indices,
                rest_outside,
                ..
            } => {
                let index = indices[self.pattern_position];
                self.pattern_position = (self.pattern_position + 1) % indices.len();
                if *rest_outside && index >= ordered.len() as u64 {
                    return Ok(None);
                }
                Some((index % ordered.len() as u64) as usize)
            }
            Selection::Transition(selection) => {
                if chance.seed.is_none() {
                    return Err("transition requires an explicit seed");
                }
                chance.transition_rank(
                    selection,
                    ordered.len(),
                    position,
                    self.revision as u64,
                    &mut self.choice_count,
                )
            }
            Selection::IntervalEnvelope(selection) => {
                if chance.seed.is_none() {
                    return Err("interval envelope requires an explicit seed");
                }
                chance.interval_rank(
                    selection,
                    ordered.len(),
                    position,
                    self.revision as u64,
                    step,
                )
            }
            _ => Some(position.map_or(0, |i| (i + 1) % ordered.len())),
        };
        let Some(rank) = rank else { return Ok(None) };
        let selected = ordered[rank].clone();
        self.last_selected = Some(selected.clone());
        Ok(Some(selected))
    }

    pub fn ordered_notes(&self, selection: &Selection) -> Result<Vec<BankNote>, &'static str> {
        if self.octaves.offsets.is_empty() {
            return Err("octave offsets must not be empty");
        }
        let mut ordered = self.published.clone();
        match selection {
            Selection::Ascending | Selection::Descending => {
                let mut keyed = Vec::new();
                for reference in ordered {
                    let phrase = self.source(&reference.capture_id)?;
                    let note = phrase
                        .notes
                        .iter()
                        .find(|n| n.note_id == reference.note_id)
                        .ok_or("source note is missing")?;
                    let key = i32::from(note.key)
                        * if *selection == Selection::Descending {
                            -1
                        } else {
                            1
                        };
                    keyed.push((key, reference));
                }
                keyed.sort_by(|a, b| {
                    (a.0, &a.1.capture_id, &a.1.note_id).cmp(&(b.0, &b.1.capture_id, &b.1.note_id))
                });
                ordered = keyed.into_iter().map(|(_, r)| r).collect();
            }
            Selection::ReversePlayed => ordered.reverse(),
            _ => {}
        }
        let reverse = matches!(selection, Selection::Descending | Selection::ReversePlayed);
        Ok(self
            .octaves
            .indices(ordered.len(), reverse)
            .into_iter()
            .map(|(i, j)| BankNote {
                octave_index: j,
                ..ordered[i].clone()
            })
            .collect())
    }

    pub fn source(&self, id: &str) -> Result<&CapturedPhrase, &'static str> {
        if let Some(phrase) = &self.live_snapshot {
            if phrase.capture_id == id {
                return Ok(phrase);
            }
        }
        self.takes
            .iter()
            .find(|t| t.phrase.capture_id == id)
            .map(|t| &t.phrase)
            .ok_or("source capture is not in the bank")
    }

    /// Pitch offset of the most recently consumed index-pattern slot.
    pub fn selected_interval(&self) -> i64 {
        match &self.selection {
            Selection::IndexPattern {
                indices,
                intervals: Some(intervals),
                ..
            } => intervals[(self.pattern_position + indices.len() - 1) % indices.len()],
            _ => 0,
        }
    }
}

struct Take {
    phrase: CapturedPhrase,
    overdub: bool,
}
