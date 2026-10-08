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
    pub processed_to: Tick,
    pub capture_to: i64,
    pub inclusive: bool,
    input_events: usize,
    current_expression: bool,
}

impl HistoryArpeggiator {
    pub fn new(
        capacity: usize,
        selection: Selection,
        retrigger_on_edit: bool,
        step: Tick,
        gate: Tick,
        profile: Profile,
        current_expression: bool,
    ) -> Result<Self, &'static str> {
        if matches!(selection, Selection::Walk(_)) {
            return Err("history requires classic selection");
        }
        if matches!(selection, Selection::Alternating { .. }) {
            return Err("history requires classic selection");
        }
        if matches!(
            selection,
            Selection::IndexPattern { .. } | Selection::InsideOut | Selection::OutsideIn
        ) {
            return Err("history requires classic selection");
        }
        if matches!(selection, Selection::Shuffle { .. }) {
            return Err("history requires classic selection");
        }
        if matches!(selection, Selection::Choice { .. }) {
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
            capture_to: 0,
            inclusive: false,
            input_events: 0,
            current_expression,
        })
    }

    pub fn accept(&mut self, event: MidiEvent) -> Result<(), &'static str> {
        if self.input_events >= 1_000_000 {
            return Err("live capture reached its event limit");
        }
        let at = Tick::from_integer(event.tick);
        if at < Tick::from_integer(self.capture_to)
            || at == Tick::from_integer(self.capture_to) && self.inclusive
        {
            return Err("MIDI input arrived after its live output time");
        }
        self.capture.accept(event, None)?;
        self.input_events += 1;
        Ok(())
    }

    pub fn before(&mut self, at: Tick, tick: i64) -> Result<Vec<RealizedEvent>, &'static str> {
        self.process(at, tick, false)
    }

    pub fn advance(&mut self, at: Tick, tick: i64) -> Result<Vec<RealizedEvent>, &'static str> {
        self.process(at, tick, true)
    }

    pub fn clear(&mut self, at: Tick, tick: i64) -> Result<Vec<RealizedEvent>, &'static str> {
        let mut events = if at == self.processed_to && self.inclusive {
            Vec::new()
        } else {
            self.before(at, tick)?
        };
        self.queue.clear();
        if let Some((source_note, key)) = self.sounding.take() {
            events.push(RealizedEvent {
                at,
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

    pub fn pause(&mut self, at: Tick, tick: i64) -> Vec<RealizedEvent> {
        self.queue.clear();
        let output = self
            .sounding
            .take()
            .map_or(Vec::new(), |(source_note, key)| {
                vec![RealizedEvent {
                    at,
                    data: vec![0x80, key, 0],
                    source_note,
                    source_event: None,
                }]
            });
        self.processed_to = at;
        self.capture_to = tick;
        self.inclusive = true;
        self.next_step = self.next_step.max((at / self.step).ceil() * self.step);
        output
    }

    pub fn relocate(&mut self, at: Tick, tick: i64) -> Vec<RealizedEvent> {
        let output = self.pause(at, tick);
        self.next_step = (at / self.step).ceil() * self.step;
        self.last_selected = None;
        self.inclusive = false;
        output
    }

    fn process(
        &mut self,
        through: Tick,
        tick: i64,
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
            if deadline > through {
                break;
            }
            if deadline == through && !inclusive {
                if self.queue.first().is_some_and(|e| {
                    e.at == through
                        && (e.data[0] & 0xf0 == 0x80 || e.data[0] & 0xf0 == 0x90 && e.data[2] == 0)
                }) {
                    output.push(self.queue.remove(0));
                    self.sounding = None;
                }
                break;
            }
            if self.next_step == deadline {
                let source_at = if through == self.processed_to {
                    Tick::from_integer(tick)
                } else {
                    Tick::from_integer(self.capture_to)
                        + Tick::from_integer(tick - self.capture_to)
                            * (deadline - self.processed_to)
                            / (through - self.processed_to)
                };
                self.play_step(deadline, source_at.floor().to_integer(), &mut output)?;
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
        self.capture_to = tick;
        self.processed_to = through;
        self.inclusive = inclusive;
        Ok(output)
    }

    fn play_step(
        &mut self,
        at: Tick,
        source_tick: i64,
        output: &mut Vec<RealizedEvent>,
    ) -> Result<(), &'static str> {
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
        self.queue.extend(
            events
                .into_iter()
                .filter(|e| !self.current_expression || matches!(e.data[0] & 0xf0, 0x80 | 0x90)),
        );
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
            Selection::Walk(_)
            | Selection::Alternating { .. }
            | Selection::Choice { .. }
            | Selection::Shuffle { .. }
            | Selection::IndexPattern { .. }
            | Selection::InsideOut
            | Selection::OutsideIn => {
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
