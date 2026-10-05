//! Stepwise history selection and owned MIDI output for completed gestures.

use crate::{
    Selection,
    capture::{CapturedPhrase, MidiCapture, MidiEvent, Profile, SourceNote, Timebase},
    gesture::{self, OverlapPolicy, Placement, RealizedEvent, Tick, Timing},
};

pub struct HistoryArpeggiator {
    capture: MidiCapture,
    history: Vec<SourceNote>,
    capacity: usize,
    selection: Selection,
    retrigger_on_edit: bool,
    step: Tick,
    gate: Tick,
    next_step: Tick,
    queue: Vec<RealizedEvent>,
    sounding: Option<(String, u8)>,
    last_selected: Option<String>,
    pub bank_revision: usize,
    processed_to: Tick,
    inclusive: bool,
    input_events: usize,
}

impl HistoryArpeggiator {
    pub fn new(
        capacity: usize,
        selection: Selection,
        retrigger_on_edit: bool,
        step: Tick,
        gate: Tick,
        profile: Profile,
    ) -> Result<Self, &'static str> {
        if matches!(selection, Selection::Walk(_)) {
            return Err("history requires classic selection");
        }
        if matches!(selection, Selection::Alternating { .. }) {
            return Err("history requires classic selection");
        }
        if capacity == 0 || step <= Tick::from_integer(0) || gate < Tick::from_integer(0) {
            return Err("history requires positive capacity and step, and nonnegative gate");
        }
        Ok(Self {
            capture: MidiCapture::new(
                "live",
                Timebase {
                    name: "microseconds".into(),
                    rate_numerator: 1_000_000,
                    rate_denominator: 1,
                },
                profile,
            )?,
            history: Vec::new(),
            capacity,
            selection,
            retrigger_on_edit,
            step,
            gate,
            next_step: Tick::from_integer(0),
            queue: Vec::new(),
            sounding: None,
            last_selected: None,
            bank_revision: 0,
            processed_to: Tick::from_integer(0),
            inclusive: false,
            input_events: 0,
        })
    }

    pub fn accept(&mut self, event: MidiEvent) -> Result<(), &'static str> {
        if self.input_events >= 1_000_000 {
            return Err("live capture reached its event limit");
        }
        let at = Tick::from_integer(event.tick);
        if at < self.processed_to || at == self.processed_to && self.inclusive {
            return Err("MIDI input arrived after its live output time");
        }
        self.capture.accept(event, None)?;
        self.input_events += 1;
        Ok(())
    }

    pub fn before(&mut self, tick: i64) -> Result<Vec<RealizedEvent>, &'static str> {
        self.process(Tick::from_integer(tick), false)
    }

    pub fn advance(&mut self, tick: i64) -> Result<Vec<RealizedEvent>, &'static str> {
        self.process(Tick::from_integer(tick), true)
    }

    pub fn clear(&mut self, tick: i64) -> Result<Vec<RealizedEvent>, &'static str> {
        let mut events = if Tick::from_integer(tick) == self.processed_to && self.inclusive {
            Vec::new()
        } else {
            self.before(tick)?
        };
        self.queue.clear();
        if let Some((source_note, key)) = self.sounding.take() {
            events.push(RealizedEvent {
                at: Tick::from_integer(tick),
                data: vec![0x80, key, 0],
                source_note,
                source_event: None,
            });
        }
        if !self.history.is_empty() {
            self.history.clear();
            self.last_selected = None;
            self.bank_revision += 1;
        }
        Ok(events)
    }

    fn process(
        &mut self,
        through: Tick,
        inclusive: bool,
    ) -> Result<Vec<RealizedEvent>, &'static str> {
        if through < self.processed_to
            || through == self.processed_to && self.inclusive && !inclusive
        {
            return Err("live time must not go backwards");
        }
        let mut output = Vec::new();
        loop {
            let deadline = self
                .queue
                .first()
                .map_or(self.next_step, |event| self.next_step.min(event.at));
            if deadline > through || deadline == through && !inclusive {
                break;
            }
            if self.next_step == deadline {
                self.play_step(deadline, &mut output)?;
                self.next_step += self.step;
            } else {
                let event = self.queue.remove(0);
                if event.data[0] & 0xf0 == 0x90 && event.data[2] > 0 {
                    self.sounding = Some((event.source_note.clone(), event.data[1]));
                } else if event.data[0] & 0xf0 == 0x80
                    || event.data[0] & 0xf0 == 0x90 && event.data[2] == 0
                {
                    self.sounding = None;
                }
                output.push(event);
            }
        }
        self.processed_to = through;
        self.inclusive = inclusive;
        Ok(output)
    }

    fn play_step(&mut self, at: Tick, output: &mut Vec<RealizedEvent>) -> Result<(), &'static str> {
        let source_tick = at.floor().to_integer();
        let ready = self.capture.advance(source_tick)?;
        if !ready.is_empty() {
            self.history.extend(ready);
            if self.history.len() > self.capacity {
                self.history.drain(..self.history.len() - self.capacity);
            }
            self.bank_revision += 1;
            if self.retrigger_on_edit {
                self.last_selected = None;
            }
        }
        let Some(note) = self.select_note() else {
            return Ok(());
        };
        self.queue.clear();
        if let Some((source_note, key)) = self.sounding.take() {
            output.push(RealizedEvent {
                at,
                data: vec![0x80, key, 0],
                source_note,
                source_event: None,
            });
        }
        let phrase: CapturedPhrase = self.capture.snapshot(source_tick)?;
        let events = gesture::render(
            &phrase,
            &[Placement {
                note_id: note.note_id.clone(),
                onset: at,
                gate: Some(self.step * self.gate),
            }],
            &[0],
            Timing::Fit,
            OverlapPolicy::Handoff,
        )?;
        self.queue.extend(events);
        Ok(())
    }

    fn select_note(&mut self) -> Option<SourceNote> {
        let mut ordered = self.history.clone();
        match self.selection {
            Selection::Ascending => {
                ordered.sort_by_key(|note| (i32::from(note.key), note.note_id.clone()))
            }
            Selection::Descending => {
                ordered.sort_by_key(|note| (-i32::from(note.key), note.note_id.clone()))
            }
            Selection::Played => {}
            Selection::ReversePlayed => ordered.reverse(),
            Selection::Walk(_) | Selection::Alternating { .. } => {
                unreachable!("validated classic selection")
            }
        }
        let position = self
            .last_selected
            .as_ref()
            .and_then(|id| ordered.iter().position(|note| &note.note_id == id));
        let note = ordered
            .get(position.map_or(0, |position| (position + 1) % ordered.len()))?
            .clone();
        self.last_selected = Some(note.note_id.clone());
        Some(note)
    }
}
