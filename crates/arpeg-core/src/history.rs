//! Stepwise history and phrase selection with owned MIDI output for completed gestures.

use crate::{
    Selection,
    bank::{CaptureBank, CaptureMode},
    capture::{MidiEvent, Profile},
    chance::Chance,
    gesture::{self, OverlapPolicy, Placement, RealizedEvent, Tick, Timing},
    ports::{InputPort, PerformancePorts},
};

pub struct HistoryArpeggiator {
    pub bank: CaptureBank,
    capture_profile: Profile,
    next_capture_id: u64,
    step: Tick,
    pub ports: PerformancePorts,
    pub chance: Chance,
    step_index: i64,
    decision_count: u64,
    next_step: Tick,
    queue: Vec<RealizedEvent>,
    sounding: Option<(String, u8)>,
    pub processed_to: Tick,
    pub capture_to: i64,
    pub inclusive: bool,
    input_events: usize,
    current_expression: bool,
}

impl HistoryArpeggiator {
    pub fn new(
        mode: CaptureMode,
        selection: Selection,
        retrigger_on_edit: bool,
        step: Tick,
        gate: Tick,
        profile: Profile,
        current_expression: bool,
    ) -> Result<Self, &'static str> {
        if step <= Tick::from_integer(0) || gate < Tick::from_integer(0) {
            return Err("capture playback requires positive step and nonnegative gate");
        }
        Ok(Self {
            bank: CaptureBank::new(mode, selection, retrigger_on_edit, profile)?,
            capture_profile: profile,
            next_capture_id: 0,
            step,
            ports: PerformancePorts::new(gate, Tick::from_integer(1)),
            chance: Chance::default(),
            step_index: 0,
            decision_count: 0,
            next_step: Tick::from_integer(0),
            queue: Vec::new(),
            sounding: None,
            processed_to: Tick::from_integer(0),
            capture_to: 0,
            inclusive: false,
            input_events: 0,
            current_expression,
        })
    }

    pub fn accept(&mut self, event: MidiEvent) -> Result<(), &'static str> {
        if self.bank.mode == CaptureMode::Phrase && self.bank.recording.is_none() {
            return Ok(());
        }
        if self.input_events >= 1_000_000 {
            return Err("live capture reached its event limit");
        }
        let at = Tick::from_integer(event.tick);
        if at < Tick::from_integer(self.capture_to)
            || at == Tick::from_integer(self.capture_to) && self.inclusive
        {
            return Err("MIDI input arrived after its live output time");
        }
        self.bank.accept(event)?;
        self.input_events += 1;
        Ok(())
    }

    pub fn before(&mut self, at: Tick, tick: i64) -> Result<Vec<RealizedEvent>, &'static str> {
        self.process(at, tick, false)
    }

    pub fn advance(&mut self, at: Tick, tick: i64) -> Result<Vec<RealizedEvent>, &'static str> {
        self.process(at, tick, true)
    }

    pub fn control(
        &mut self,
        at: Tick,
        tick: i64,
        port: InputPort,
        value: Tick,
    ) -> Result<Vec<RealizedEvent>, &'static str> {
        PerformancePorts::check_control(port, value, self.chance.seed)?;
        if at < self.processed_to {
            return Err("live time must not go backwards");
        }
        let output = if at > self.processed_to {
            self.before(at, tick)?
        } else {
            Vec::new()
        };
        self.ports.queue(port, value);
        Ok(output)
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
        if self.bank.mode == CaptureMode::Phrase {
            self.bank.clear();
            self.input_events = 0;
        } else {
            self.bank.clear_history()?;
        }
        Ok(events)
    }

    pub fn check_control(&self, command: &str) -> Result<(), &'static str> {
        if self.bank.mode != CaptureMode::Phrase {
            return Err("capture controls require a phrase bank");
        }
        self.bank.check_control(command)
    }

    pub fn capture(
        &mut self,
        command: &str,
        at: Tick,
        tick: i64,
    ) -> Result<Vec<RealizedEvent>, &'static str> {
        self.check_control(command)?;
        let events = if at > self.processed_to {
            self.before(at, tick)?
        } else {
            Vec::new()
        };
        match command {
            "record" => {
                self.bank.record(
                    &format!("take-{}", self.next_capture_id),
                    self.capture_profile,
                )?;
                self.next_capture_id = self
                    .next_capture_id
                    .checked_add(1)
                    .ok_or("capture identity limit reached")?;
            }
            "undo" => self.bank.undo()?,
            _ => self.bank.commit(tick, command == "overdub")?,
        }
        self.capture_to = tick;
        self.inclusive = false;
        Ok(events)
    }

    pub fn check_record_budget(&self, prefix_events: usize) -> Result<(), &'static str> {
        if self.input_events + prefix_events > 1_000_000 {
            Err("live capture reached its event limit")
        } else {
            Ok(())
        }
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
        self.step_index = (self.next_step / self.step).ceil().to_integer();
        output
    }

    pub fn relocate(&mut self, at: Tick, tick: i64) -> Vec<RealizedEvent> {
        let output = self.pause(at, tick);
        self.next_step = (at / self.step).ceil() * self.step;
        self.step_index = (at / self.step).ceil().to_integer();
        self.ports.cancel_pending();
        self.bank.last_selected = None;
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
                self.step_index += 1;
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
        self.bank.advance(source_tick)?;
        self.bank.publish_step();
        if !self
            .ports
            .begin_step(at, self.step_index, self.bank.revision as u64)
        {
            return Ok(());
        }
        if self.bank.published.is_empty() {
            self.bank.last_selected = None;
            self.ports.outcome(false);
            return Ok(());
        }
        let index = self.decision_count;
        self.decision_count += 1;
        if !self
            .chance
            .allows(self.bank.revision as u64, index, self.ports.density)
        {
            self.ports.outcome(false);
            return Ok(());
        }
        let Some(note) = self.bank.select_step()? else {
            return Ok(());
        };
        let note = if self.ports.selection_offset != 0 {
            let mut ranked = Vec::new();
            for reference in &self.bank.published {
                let source = self
                    .bank
                    .source(&reference.capture_id)?
                    .notes
                    .iter()
                    .find(|n| n.note_id == reference.note_id)
                    .ok_or("source note is missing")?;
                ranked.push((source.key, reference));
            }
            ranked.sort_by(|a, b| {
                (a.0, &a.1.capture_id, &a.1.note_id).cmp(&(b.0, &b.1.capture_id, &b.1.note_id))
            });
            let rank = ranked.iter().position(|(_, r)| **r == note).unwrap();
            let Some(rank) = self.ports.offset_rank(rank, ranked.len()) else {
                self.ports.outcome(false);
                return Ok(());
            };
            ranked[rank].1.clone()
        } else {
            note
        };
        let phrase = self.bank.source(&note.capture_id)?;
        let source = phrase
            .notes
            .iter()
            .find(|n| n.note_id == note.note_id)
            .ok_or("missing captured note")?;
        let Some(key) = self.ports.realize_pitch(source.key)? else {
            self.ports.outcome(false);
            return Ok(());
        };
        self.ports.outcome(true);
        self.queue.clear();
        if let Some((source_note, key)) = self.sounding.take() {
            output.push(RealizedEvent {
                at,
                data: vec![0x80, key, 0],
                source_note,
                source_event: None,
            });
        }
        let phrase = self.bank.source(&note.capture_id)?;
        let events = gesture::render(
            phrase,
            &[Placement {
                note_id: note.note_id.clone(),
                onset: at,
                gate: Some(self.step * self.ports.gate),
            }],
            &[0],
            Timing::Fit,
            OverlapPolicy::Handoff,
        )?;
        self.queue.extend(
            events
                .into_iter()
                .map(|mut e| {
                    if matches!(e.data[0] & 0xf0, 0x80 | 0x90) {
                        e.data[1] = key;
                    }
                    e.source_note = format!("{}:{}", note.capture_id, e.source_note);
                    e
                })
                .filter(|e| !self.current_expression || matches!(e.data[0] & 0xf0, 0x80 | 0x90)),
        );
        Ok(())
    }
}
