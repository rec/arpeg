//! Lossless MIDI note capture with explicit monophonic overlap policy.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Overlap {
    Handoff,
    Independent,
}

#[derive(Clone, Copy, Debug)]
pub struct Profile {
    pub channel: u8,
    pub overlap: Overlap,
    pub breath_cc: u8,
    pub track_bend: bool,
    pub tail_ticks: i64,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            channel: 0,
            overlap: Overlap::Handoff,
            breath_cc: 2,
            track_bend: true,
            tail_ticks: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MidiEvent {
    pub tick: i64,
    pub ordinal: u32,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Timebase {
    pub name: String,
    pub rate_numerator: i64,
    pub rate_denominator: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EntryState {
    pub value: Option<f64>,
    pub source_event: Option<usize>,
}

impl EntryState {
    fn unknown() -> Self {
        Self {
            value: None,
            source_event: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceNote {
    pub capture_id: String,
    pub note_id: String,
    pub onset_tick: i64,
    pub gate_end_tick: i64,
    pub cell_end_tick: i64,
    pub onset_event: usize,
    pub release_event: Option<usize>,
    pub expression_events: Vec<usize>,
    pub following_events: Vec<usize>,
    pub entry_state: BTreeMap<String, EntryState>,
    pub key: u8,
    pub velocity: f64,
    pub release_velocity: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CapturedPhrase {
    pub capture_id: String,
    pub timebase: Timebase,
    pub end_tick: i64,
    pub events: Vec<MidiEvent>,
    pub notes: Vec<SourceNote>,
    pub prefix_events: Vec<usize>,
}

#[derive(Clone, Debug)]
struct Segment {
    note_id: String,
    onset_tick: i64,
    gate_end_tick: Option<i64>,
    cell_end_tick: Option<i64>,
    onset_event: usize,
    release_event: Option<usize>,
    expression_events: Vec<usize>,
    following_events: Vec<usize>,
    entry_state: BTreeMap<String, EntryState>,
    key: u8,
    velocity: u8,
    release_velocity: Option<u8>,
}

pub struct MidiCapture {
    capture_id: String,
    timebase: Timebase,
    profile: Profile,
    events: Vec<MidiEvent>,
    segments: Vec<Segment>,
    prefix_events: Vec<usize>,
    controller_state: BTreeMap<String, EntryState>,
    active: Vec<usize>,
    reported_notes: Vec<String>,
    advanced_through: Option<i64>,
}

impl MidiCapture {
    pub fn new(
        capture_id: &str,
        timebase: Timebase,
        profile: Profile,
    ) -> Result<Self, &'static str> {
        if profile.channel > 15 || profile.breath_cc > 127 || profile.tail_ticks < 0 {
            return Err("invalid MIDI capture profile");
        }
        if timebase.rate_numerator <= 0 || timebase.rate_denominator <= 0 {
            return Err("invalid capture timebase");
        }
        let mut controller_state = BTreeMap::new();
        controller_state.insert("breath".into(), EntryState::unknown());
        if profile.track_bend {
            controller_state.insert("bend".into(), EntryState::unknown());
        }
        Ok(Self {
            capture_id: capture_id.into(),
            timebase,
            profile,
            events: Vec::new(),
            segments: Vec::new(),
            prefix_events: Vec::new(),
            controller_state,
            active: Vec::new(),
            reported_notes: Vec::new(),
            advanced_through: None,
        })
    }

    pub fn accept(&mut self, event: MidiEvent, note_id: Option<&str>) -> Result<(), &'static str> {
        if self.advanced_through.is_some_and(|tick| event.tick <= tick) {
            return Err("MIDI event arrived after its capture time was published");
        }
        if event.tick < 0
            || self
                .events
                .last()
                .is_some_and(|last| (event.tick, event.ordinal) <= (last.tick, last.ordinal))
        {
            return Err("capture events must increase in (tick, ordinal) order");
        }
        let status = *event.data.first().ok_or("empty MIDI event")?;
        let kind = status & 0xf0;
        let is_onset = (0x80..0xf0).contains(&status)
            && status & 15 == self.profile.channel
            && kind == 0x90
            && event.data.len() == 3
            && event.data[2] > 0;
        let note_id = note_id
            .map(str::to_owned)
            .unwrap_or_else(|| format!("note-{}", self.segments.len()));
        if is_onset {
            if self.segments.iter().any(|note| note.note_id == note_id) {
                return Err("duplicate source note ID");
            }
            if self.profile.overlap == Overlap::Handoff
                && self
                    .segments
                    .last()
                    .is_some_and(|last| last.onset_tick == event.tick)
            {
                return Err("simultaneous onsets require independent overlap");
            }
        }
        let index = self.events.len();
        self.events.push(event);
        if !(0x80..0xf0).contains(&status) || status & 15 != self.profile.channel {
            self.retain_context(index);
            return Ok(());
        }
        if self.events[index].data.len() != 3 {
            if kind == 0xd0 && self.events[index].data.len() == 2 {
                self.controller_state.insert(
                    "pressure".into(),
                    EntryState {
                        value: Some(f64::from(self.events[index].data[1]) / 127.0),
                        source_event: Some(index),
                    },
                );
                self.expression_or_context(index);
                return Ok(());
            }
            self.retain_context(index);
            return Ok(());
        }
        let first = self.events[index].data[1];
        let value = self.events[index].data[2];
        if is_onset {
            self.onset(index, first, value, note_id);
        } else if kind == 0x80 || kind == 0x90 && value == 0 {
            self.release(index, first, value);
        } else if kind == 0xb0 && first == self.profile.breath_cc {
            self.controller_state.insert(
                "breath".into(),
                EntryState {
                    value: Some(f64::from(value) / 127.0),
                    source_event: Some(index),
                },
            );
            self.expression_or_context(index);
        } else if kind == 0xe0 && self.profile.track_bend {
            let bend = i32::from(first) | (i32::from(value) << 7);
            self.controller_state.insert(
                "bend".into(),
                EntryState {
                    value: Some(f64::from(bend - 8192) / 8192.0),
                    source_event: Some(index),
                },
            );
            self.expression_or_context(index);
        } else {
            self.retain_context(index);
        }
        Ok(())
    }

    pub fn advance(&mut self, through_tick: i64) -> Result<Vec<SourceNote>, &'static str> {
        if through_tick < self.events.last().map_or(0, |event| event.tick)
            || self
                .advanced_through
                .is_some_and(|tick| through_tick < tick)
        {
            return Err("capture time precedes source events");
        }
        self.advanced_through = Some(through_tick);
        let mut ready = Vec::new();
        for (index, segment) in self.segments.iter_mut().enumerate() {
            let Some(gate) = segment.gate_end_tick else {
                continue;
            };
            if through_tick < gate + self.profile.tail_ticks {
                continue;
            }
            if segment.cell_end_tick.is_none() && !self.active.contains(&index) {
                segment.cell_end_tick =
                    Some((gate + self.profile.tail_ticks).max(segment.onset_tick + 1));
                segment
                    .following_events
                    .retain(|i| Some(self.events[*i].tick) < segment.cell_end_tick);
            }
            if segment.cell_end_tick.is_some() && !self.reported_notes.contains(&segment.note_id) {
                ready.push(source_note(segment, &self.capture_id));
                self.reported_notes.push(segment.note_id.clone());
            }
        }
        Ok(ready)
    }

    pub fn snapshot(&mut self, through_tick: i64) -> Result<CapturedPhrase, &'static str> {
        self.advance(through_tick)?;
        Ok(CapturedPhrase {
            capture_id: self.capture_id.clone(),
            timebase: self.timebase.clone(),
            end_tick: through_tick,
            events: self.events.clone(),
            notes: self
                .segments
                .iter()
                .filter(|segment| self.reported_notes.contains(&segment.note_id))
                .map(|segment| source_note(segment, &self.capture_id))
                .collect(),
            prefix_events: self.prefix_events.clone(),
        })
    }

    pub fn finish(mut self, end_tick: i64) -> Result<CapturedPhrase, &'static str> {
        if end_tick < self.events.last().map_or(0, |event| event.tick)
            || self.advanced_through.is_some_and(|tick| end_tick < tick)
        {
            return Err("phrase end precedes source events");
        }
        for segment in &mut self.segments {
            segment.gate_end_tick.get_or_insert(end_tick);
            segment.cell_end_tick.get_or_insert(end_tick);
            if segment.cell_end_tick <= Some(segment.onset_tick) {
                return Err("cell end must follow onset");
            }
        }
        let notes = self
            .segments
            .iter()
            .map(|segment| source_note(segment, &self.capture_id))
            .collect();
        Ok(CapturedPhrase {
            capture_id: self.capture_id,
            timebase: self.timebase,
            end_tick,
            events: self.events,
            notes,
            prefix_events: self.prefix_events,
        })
    }

    fn onset(&mut self, index: usize, key: u8, velocity: u8, note_id: String) {
        let tick = self.events[index].tick;
        if self.profile.overlap == Overlap::Handoff {
            for active in &self.active {
                self.segments[*active].gate_end_tick = Some(tick);
            }
            self.active.clear();
            if let Some(last) = self.segments.last_mut() {
                last.cell_end_tick.get_or_insert(tick);
            }
        }
        self.segments.push(Segment {
            note_id,
            onset_tick: tick,
            gate_end_tick: None,
            cell_end_tick: None,
            onset_event: index,
            release_event: None,
            expression_events: Vec::new(),
            following_events: Vec::new(),
            entry_state: self.controller_state.clone(),
            key,
            velocity,
            release_velocity: None,
        });
        self.active.push(self.segments.len() - 1);
    }

    fn release(&mut self, index: usize, key: u8, velocity: u8) {
        if let Some(position) = self
            .active
            .iter()
            .position(|active| self.segments[*active].key == key)
        {
            let segment = &mut self.segments[self.active.remove(position)];
            segment.gate_end_tick = Some(self.events[index].tick);
            segment.release_event = Some(index);
            segment.release_velocity = Some(velocity);
        } else {
            self.retain_context(index);
        }
    }

    fn expression_or_context(&mut self, index: usize) {
        if self.active.is_empty() {
            if !self.segments.is_empty() {
                self.retain_context(index);
            }
        } else {
            for active in &self.active {
                self.segments[*active].expression_events.push(index);
            }
        }
    }

    fn retain_context(&mut self, index: usize) {
        if self.segments.is_empty() {
            self.prefix_events.push(index);
        } else if self.active.is_empty() {
            if let Some(last) = self.segments.last_mut() {
                if last.cell_end_tick.is_none() {
                    last.following_events.push(index);
                }
            }
        }
    }
}

fn source_note(segment: &Segment, capture_id: &str) -> SourceNote {
    SourceNote {
        capture_id: capture_id.into(),
        note_id: segment.note_id.clone(),
        onset_tick: segment.onset_tick,
        gate_end_tick: segment.gate_end_tick.expect("complete gate"),
        cell_end_tick: segment.cell_end_tick.expect("complete cell"),
        onset_event: segment.onset_event,
        release_event: segment.release_event,
        expression_events: segment.expression_events.clone(),
        following_events: segment.following_events.clone(),
        entry_state: segment.entry_state.clone(),
        key: segment.key,
        velocity: f64::from(segment.velocity) / 127.0,
        release_velocity: segment
            .release_velocity
            .map(|value| f64::from(value) / 127.0),
    }
}
