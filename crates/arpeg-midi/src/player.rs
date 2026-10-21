//! Device-independent MIDI player shared by host execution and conformance tests.

use arpeg_core::{
    capture::{MidiEvent, Profile as CaptureProfile},
    clock::{ClockMode, TransportClock},
    gesture::RealizedEvent,
    history::HistoryArpeggiator,
    live::{LiveArpeggiator, OutputEvent, OutputKind},
    ports::{InputPort, NoteEvent, NotePort, PerformancePorts, PortBatch},
};

use crate::{
    HeldProfile, Profile,
    expression::{self, Lane, Source},
};

#[derive(Clone, Copy)]
pub enum InputSource {
    Notes,
    Clock,
    Both,
}

fn prepare_lane(p: HeldProfile) -> Result<LiveArpeggiator, &'static str> {
    let mut engine =
        LiveArpeggiator::musical(p.bank, p.selection, p.rhythm, p.gate, p.retrigger, p.chance)?;
    engine.patterns = p.patterns.unwrap_or_default();
    engine.ports.channel = p.channel;
    engine.ports.strum_spacing = p.strum_spacing;
    engine.ports.transposition = p.transposition;
    engine.ports.pitch_boundary = p.pitch_boundary;
    engine.ports.register_limits = p.register_limits;
    engine.ports.scale = p.scale;
    engine.ports.tuning = p.tuning;
    engine.ports.selection_offset = p.selection_offset;
    engine.ports.offset_rest_outside = p.offset_rest_outside;
    engine.ports.octaves = p.octaves;
    engine.ports.inversion = p.inversion;
    engine.ports.voicing = p.voicing;
    engine.lanes = p
        .lanes
        .into_iter()
        .map(prepare_lane)
        .collect::<Result<_, _>>()?;
    Ok(engine)
}

enum Engine {
    Held(Box<LiveArpeggiator>),
    History(Box<HistoryArpeggiator>),
}

struct ScheduledMidi {
    active: bool,
    notes: Vec<OutputEvent>,
    history: Vec<RealizedEvent>,
    release: bool,
}

#[derive(Clone)]
struct ChannelSettings {
    bend_range: Option<f64>,
    sources: std::collections::BTreeMap<Lane, Source>,
    strum: bool,
}

pub struct MidiPlayer {
    channel: u8,
    channels: std::collections::BTreeMap<u8, ChannelSettings>,
    channel_offsets: std::collections::BTreeMap<u8, f64>,
    pub clock: TransportClock,
    engine: Engine,
    origin_us: Option<i64>,
    wall_us: i64,
    input_tick: i64,
    ordinal: u32,
    scheduled: Option<ScheduledMidi>,
    expression: std::collections::BTreeMap<u8, Vec<u8>>,
    envelfo_expression: std::collections::BTreeMap<Lane, arpeg_core::Beat>,
    sounding: std::collections::BTreeMap<(u8, u64), NoteEvent>,
    next_occurrence: u64,
    notes: Vec<NoteEvent>,
    notes_exhausted: bool,
    suppressed_note: bool,
}

impl MidiPlayer {
    pub fn new(
        profile: Profile,
        mode: ClockMode,
        bpm: u32,
        timeout_us: i64,
    ) -> Result<Self, &'static str> {
        let mut channels = std::collections::BTreeMap::new();
        let channel = match &profile {
            Profile::Classic(p) => {
                for lane in std::iter::once(p).chain(&p.lanes) {
                    channels.insert(
                        lane.channel,
                        ChannelSettings {
                            bend_range: lane.bend_range,
                            sources: lane.expression_sources.clone(),
                            strum: lane.strum_spacing.is_some(),
                        },
                    );
                }
                p.channel
            }
            Profile::Captured(p) => {
                channels.insert(
                    p.channel,
                    ChannelSettings {
                        bend_range: p.bend_range,
                        sources: p.expression_sources.clone(),
                        strum: false,
                    },
                );
                p.channel
            }
        };
        let engine = match profile {
            Profile::Classic(p) => Engine::Held(Box::new(prepare_lane(p)?)),
            Profile::Captured(p) => {
                let mut engine = HistoryArpeggiator::new(
                    p.mode,
                    p.selection,
                    p.retrigger == arpeg_core::live::Retrigger::BankEdit,
                    p.step,
                    p.gate,
                    CaptureProfile::default(),
                    false,
                )?;
                engine.swing = p.swing;
                engine.groove = p.groove;
                engine.chance = p.chance;
                engine.patterns = p.patterns.unwrap_or_default();
                engine.ports.transposition = p.transposition;
                engine.ports.pitch_boundary = p.pitch_boundary;
                engine.ports.register_limits = p.register_limits;
                engine.ports.scale = p.scale;
                engine.ports.tuning = p.tuning;
                engine.ports.selection_offset = p.selection_offset;
                engine.ports.offset_rest_outside = p.offset_rest_outside;
                engine.bank.octaves = p.octaves;
                Engine::History(Box::new(engine))
            }
        };
        Ok(Self {
            channel,
            channels,
            channel_offsets: std::collections::BTreeMap::new(),
            clock: TransportClock::new(mode, bpm, timeout_us)?,
            engine,
            origin_us: None,
            wall_us: 0,
            input_tick: -1,
            ordinal: 0,
            scheduled: None,
            expression: std::collections::BTreeMap::new(),
            envelfo_expression: std::collections::BTreeMap::new(),
            sounding: std::collections::BTreeMap::new(),
            next_occurrence: 0,
            notes: Vec::new(),
            notes_exhausted: false,
            suppressed_note: false,
        })
    }

    pub fn accept(
        &mut self,
        at_us: i64,
        data: &[u8],
        source: InputSource,
    ) -> Result<Vec<Vec<u8>>, &'static str> {
        self.check_ready()?;
        let transport = matches!(data[0], 0xf8 | 0xfa | 0xfb | 0xfc | 0xf2);
        if transport {
            if matches!(source, InputSource::Notes)
                || data[0] == 0xf8 && self.clock.mode == ClockMode::Internal
            {
                return Ok(Vec::new());
            }
            let tick = self.elapsed(at_us);
            if self.clock.accept(tick, data)? {
                let at = self.clock.beat;
                return Ok(match &mut self.engine {
                    Engine::Held(e) => {
                        let events = e.relocate(at);
                        self.note_messages(events)?
                    }
                    Engine::History(e) => {
                        let events = e.relocate(at, tick);
                        self.history_messages(events)?
                    }
                });
            }
            return Ok(if self.clock.active() {
                Vec::new()
            } else {
                self.pause(tick)?
            });
        }
        if matches!(source, InputSource::Clock) {
            return Ok(Vec::new());
        }
        let expressive = matches!(data[0], 0xd0 | 0xe0) || data[0] == 0xb0 && data[1] == 2;
        if matches!(self.engine, Engine::Held(_))
            && !expressive
            && (data.len() != 3 || !matches!(data[0], 0x80 | 0x90))
        {
            return Ok(Vec::new());
        }
        let tick = self.elapsed(at_us);
        let was_active = self.clock.active();
        let at = self.clock.advance(tick);
        let mut output = if was_active && !self.clock.active() {
            self.pause(tick)?
        } else {
            Vec::new()
        };
        match &mut self.engine {
            Engine::Held(e) => {
                if expressive {
                    let events = e.before(at)?;
                    output.extend(self.note_messages(events)?);
                    self.expression.insert(data[0], data.to_vec());
                    if self.clock.active() {
                        output.extend(self.expression_messages(data, Source::Current, &[])?);
                    }
                    return Ok(output);
                }
                let events = if data[0] == 0x90 && data[2] > 0 {
                    e.note_on(at, data[1], data[2])?
                } else {
                    e.note_off(at, data[1])?
                };
                output.extend(self.note_messages(events)?);
            }
            Engine::History(e) => {
                let tick = tick
                    .max(e.capture_to + i64::from(e.inclusive))
                    .max(self.input_tick);
                let events = if self.clock.active() && at > e.processed_to {
                    e.before(at, tick)?
                } else {
                    Vec::new()
                };
                if tick != self.input_tick {
                    self.input_tick = tick;
                    self.ordinal = 0;
                }
                e.accept(MidiEvent {
                    tick,
                    ordinal: self.ordinal,
                    data: data.to_vec(),
                })?;
                self.ordinal = self
                    .ordinal
                    .checked_add(1)
                    .ok_or("too many simultaneous MIDI messages")?;
                output.extend(self.history_messages(events)?);
                if expressive {
                    self.expression.insert(data[0], data.to_vec());
                    if self.clock.active() {
                        output.extend(self.expression_messages(data, Source::Current, &[])?);
                    }
                }
            }
        }
        Ok(output)
    }

    pub fn advance(&mut self, at_us: i64) -> Result<Vec<Vec<u8>>, &'static str> {
        self.schedule(at_us, None)?;
        self.realize(&[])
    }

    /// Advance once without constructing MIDI. Drain hits, observe expression,
    /// then call `realize`. Hosts must visit each attack boundary in order.
    /// A tempo change applies after settling this boundary, as in `set_tempo`.
    pub fn schedule(&mut self, at_us: i64, tempo: Option<u32>) -> Result<(), &'static str> {
        self.check_ready()?;
        if tempo.is_some_and(|bpm| bpm == 0 || bpm > 1000 || self.clock.mode != ClockMode::Internal)
        {
            return Err("tempo requires internal clock and BPM between 1 and 1000");
        }
        let mut scheduled = ScheduledMidi {
            active: self.clock.active(),
            notes: Vec::new(),
            history: Vec::new(),
            release: false,
        };
        if self.origin_us.is_some() {
            let tick = self.elapsed(at_us);
            let was_active = self.clock.active();
            let at = self.clock.advance(tick);
            scheduled.release = was_active && !self.clock.active();
            if scheduled.release {
                match &mut self.engine {
                    Engine::Held(e) => scheduled.notes = e.pause(at),
                    Engine::History(e) => scheduled.history = e.pause(at, tick),
                }
            } else if self.clock.active() {
                match &mut self.engine {
                    Engine::Held(e) => scheduled.notes = e.advance(at)?,
                    Engine::History(e) => {
                        scheduled.history = e.advance(at, tick.max(self.input_tick))?
                    }
                }
            }
        }
        if let Some(bpm) = tempo {
            let tick = self.elapsed(at_us);
            self.clock.set_tempo(tick, bpm)?;
        }
        scheduled.active = self.clock.active();
        self.scheduled = Some(scheduled);
        Ok(())
    }

    /// Apply expression before pending attacks, or once to a continuing note.
    /// Invalid controls leave the pending batch available for correction.
    /// This does not advance clocks, generate hits, or observe an enveLFO.
    pub fn realize(
        &mut self,
        controls: &[(InputPort, arpeg_core::Beat)],
    ) -> Result<Vec<Vec<u8>>, &'static str> {
        let scheduled = self
            .scheduled
            .as_ref()
            .ok_or("schedule MIDI before realization")?;
        let samples = controls
            .iter()
            .map(|&(p, v)| self.expression_sample(p, v))
            .collect::<Result<Vec<_>, _>>()?;
        let attacks: Vec<_> = scheduled
            .notes
            .iter()
            .filter_map(|e| matches!(e.kind, OutputKind::NoteOn { .. }).then_some(e.at))
            .chain(
                scheduled
                    .history
                    .iter()
                    .filter_map(|e| (e.data[0] == 0x90 && e.data[2] > 0).then_some(e.at)),
            )
            .collect();
        if !samples.is_empty()
            && attacks
                .first()
                .is_some_and(|first| attacks.iter().any(|at| at != first))
        {
            return Err("expression requires scheduling each attack boundary");
        }
        for (lane, value, _) in &samples {
            self.envelfo_expression.insert(*lane, *value);
        }
        let scheduled = self.scheduled.take().expect("checked scheduled batch");
        let mut output = self.note_messages(scheduled.notes)?;
        output.extend(self.history_messages(scheduled.history)?);
        if scheduled.release {
            self.release_output(&mut output);
        }
        if !samples.is_empty() && scheduled.active {
            let attacked: Vec<_> = output
                .iter()
                .filter(|m| m[0] & 0xf0 == 0x90 && m[2] > 0)
                .map(|m| m[0] & 0xf)
                .collect();
            for (_, _, data) in samples {
                output.extend(self.expression_messages(&data, Source::Envelfo, &attacked)?);
            }
        }
        Ok(output)
    }

    pub fn clear(&mut self, at_us: i64) -> Result<Vec<Vec<u8>>, &'static str> {
        let mut output = self.advance(at_us)?;
        let tick = self.elapsed(at_us);
        let at = self.clock.advance(tick);
        output.extend(match &mut self.engine {
            Engine::Held(e) => {
                let events = e.clear(at)?;
                self.note_messages(events)?
            }
            Engine::History(e) => {
                let events = e.clear(at, tick)?;
                self.history_messages(events)?
            }
        });
        Ok(output)
    }

    pub fn capture(&mut self, at_us: i64, command: &str) -> Result<Vec<Vec<u8>>, &'static str> {
        self.check_ready()?;
        let Engine::History(engine) = &self.engine else {
            return Err("capture controls require a phrase bank");
        };
        engine.check_control(command)?;
        if command == "record" {
            engine.check_record_budget(self.expression.len())?;
        }
        let capture_to = engine.capture_to + i64::from(engine.inclusive);
        let tick = self.elapsed(at_us).max(self.input_tick + 1).max(capture_to);
        let was_active = self.clock.active();
        let at = self.clock.advance(self.wall_us);
        let mut output = if was_active && !self.clock.active() {
            self.pause(tick)?
        } else {
            Vec::new()
        };
        let Engine::History(engine) = &mut self.engine else {
            unreachable!()
        };
        let events = engine.capture(command, at, tick)?;
        self.input_tick = tick;
        self.ordinal = 0;
        if command == "record" {
            for status in [0xb0, 0xe0, 0xd0] {
                if let Some(data) = self.expression.get(&status) {
                    engine.accept(MidiEvent {
                        tick,
                        ordinal: self.ordinal,
                        data: data.clone(),
                    })?;
                    self.ordinal += 1;
                }
            }
        }
        output.extend(self.history_messages(events)?);
        Ok(output)
    }

    pub fn capture_state(&self) -> Option<(bool, usize, usize)> {
        match &self.engine {
            Engine::History(e) => Some((
                e.bank.recording.is_some(),
                e.bank.published.len(),
                e.bank.revision,
            )),
            Engine::Held(_) => None,
        }
    }

    pub fn control(
        &mut self,
        at_us: i64,
        port: InputPort,
        value: arpeg_core::Beat,
    ) -> Result<Vec<Vec<u8>>, &'static str> {
        self.check_ready()?;
        let sample = if let Some(lane) = expression::control_lane(port) {
            let (_, _, data) = self.expression_sample(port, value)?;
            Some((lane, data))
        } else {
            match &self.engine {
                Engine::Held(e) => {
                    PerformancePorts::check_control(port, value, e.chance.seed)?;
                    for lane in &e.lanes {
                        PerformancePorts::check_control(port, value, lane.chance.seed)?;
                    }
                }
                Engine::History(e) => PerformancePorts::check_control(port, value, e.chance.seed)?,
            }
            None
        };
        let tick = self.elapsed(at_us);
        let was_active = self.clock.active();
        let at = self.clock.advance(tick);
        let mut output = if was_active && !self.clock.active() {
            self.pause(tick)?
        } else {
            Vec::new()
        };
        if let Some((lane, data)) = sample {
            if self.clock.active() {
                match &mut self.engine {
                    Engine::Held(e) => {
                        let events = e.before(at)?;
                        output.extend(self.note_messages(events)?);
                    }
                    Engine::History(e) if at > e.processed_to => {
                        let events = e.before(at, tick.max(self.input_tick))?;
                        output.extend(self.history_messages(events)?);
                    }
                    _ => {}
                }
            }
            self.envelfo_expression.insert(lane, value);
            if self.clock.active() {
                output.extend(self.expression_messages(&data, Source::Envelfo, &[])?);
            }
            return Ok(output);
        }
        match &mut self.engine {
            Engine::Held(e) => {
                let events = e.control(at, port, value)?;
                output.extend(self.note_messages(events)?);
            }
            Engine::History(e) => {
                let events = e.control(at, tick.max(self.input_tick), port, value)?;
                output.extend(self.history_messages(events)?);
            }
        }
        Ok(output)
    }

    pub fn take_events(&mut self) -> PortBatch {
        let mut batch = match &mut self.engine {
            Engine::Held(e) => {
                let mut batch = e.ports.take_events();
                for event in &mut batch.events {
                    event.channel = e.ports.channel + 1;
                }
                for lane in &mut e.lanes {
                    let mut child = lane.ports.take_events();
                    for event in &mut child.events {
                        event.channel = lane.ports.channel + 1;
                    }
                    batch.events.extend(child.events);
                    batch.exhausted |= child.exhausted;
                }
                batch.events.sort_by_key(|e| e.at);
                batch
            }
            Engine::History(e) => {
                let mut batch = e.ports.take_events();
                for event in &mut batch.events {
                    event.channel = self.channel + 1;
                }
                batch
            }
        };
        batch.notes = std::mem::take(&mut self.notes);
        batch.exhausted |= std::mem::take(&mut self.notes_exhausted);
        batch
    }

    pub fn stop(&mut self, at_us: i64) -> Result<Vec<Vec<u8>>, &'static str> {
        let tick = self.elapsed(at_us);
        self.clock.halt(tick);
        self.pause(tick)
    }

    pub fn set_tempo(&mut self, at_us: i64, bpm: u32) -> Result<Vec<Vec<u8>>, &'static str> {
        self.schedule(at_us, Some(bpm))?;
        self.realize(&[])
    }

    fn pause(&mut self, tick: i64) -> Result<Vec<Vec<u8>>, &'static str> {
        let cancelled = self.scheduled.take().is_some();
        let mut output = match &mut self.engine {
            Engine::Held(e) => {
                let events = e.pause(self.clock.beat);
                if cancelled {
                    Vec::new()
                } else {
                    self.note_messages(events)?
                }
            }
            Engine::History(e) => {
                let events = e.pause(self.clock.beat, tick);
                if cancelled {
                    Vec::new()
                } else {
                    self.history_messages(events)?
                }
            }
        };
        self.release_output(&mut output);
        Ok(output)
    }

    fn release_output(&mut self, output: &mut Vec<Vec<u8>>) {
        let order = self.channel_order();
        let mut owners = self.sounding.keys().copied().collect::<Vec<_>>();
        owners.sort_by_key(|(channel, id)| (order.iter().position(|c| c == channel).unwrap(), *id));
        for (channel, id) in owners {
            let note = &self.sounding[&(channel, id)];
            output.push(vec![0x80 + channel, note.key, 0]);
            self.end_note(self.clock.beat, 0, channel, id);
        }
    }

    fn check_ready(&self) -> Result<(), &'static str> {
        if self.scheduled.is_some() {
            return Err("realize scheduled MIDI before changing the player");
        }
        Ok(())
    }

    fn expression_sample(
        &self,
        port: InputPort,
        value: arpeg_core::Beat,
    ) -> Result<(Lane, arpeg_core::Beat, Vec<u8>), &'static str> {
        let lane =
            expression::control_lane(port).ok_or("realization accepts only expression controls")?;
        if !self
            .channels
            .values()
            .any(|c| c.sources[&lane] == Source::Envelfo)
        {
            return Err("expression lane is not owned by Envelfo");
        }
        Ok((lane, value, expression::envelfo_message(lane, value)?))
    }

    fn expression_messages(
        &self,
        data: &[u8],
        source: Source,
        exclude: &[u8],
    ) -> Result<Vec<Vec<u8>>, &'static str> {
        let lane = expression::lane(data).expect("expressive MIDI message");
        let mut output = Vec::new();
        for channel in self.channel_order() {
            let settings = &self.channels[&channel];
            if exclude.contains(&channel)
                || settings.sources[&lane] != source
                || !self.sounding.keys().any(|(c, _)| *c == channel)
            {
                continue;
            }
            let mut wire = expression::tuning_message(
                data.to_vec(),
                self.channel_offsets.get(&channel).copied().unwrap_or(0.0),
                settings.bend_range,
            )?;
            wire[0] += channel;
            output.push(wire);
        }
        Ok(output)
    }

    fn channel_order(&self) -> Vec<u8> {
        match &self.engine {
            Engine::Held(engine) => std::iter::once(self.channel)
                .chain(engine.lanes.iter().map(|l| l.ports.channel))
                .collect(),
            Engine::History(_) => vec![self.channel],
        }
    }

    fn note_messages(&mut self, events: Vec<OutputEvent>) -> Result<Vec<Vec<u8>>, &'static str> {
        let mut output = Vec::new();
        for event in events {
            let channel = event.channel;
            let settings = self.channels[&channel].clone();
            match event.kind {
                OutputKind::NoteOn {
                    id,
                    source_id,
                    key,
                    velocity,
                    tuning_semitones,
                } => {
                    let replaced: Vec<_> = self
                        .sounding
                        .iter()
                        .filter(|((c, _), n)| *c == channel && (!settings.strum || n.key == key))
                        .map(|(k, _)| *k)
                        .collect();
                    for (c, i) in replaced {
                        output.push(vec![0x80 + channel, self.sounding[&(c, i)].key, 0]);
                        self.end_note(event.at, 0, c, i);
                    }
                    if self.sounding.keys().any(|(c, _)| *c == channel)
                        && self.channel_offsets.get(&channel).copied().unwrap_or(0.0)
                            != tuning_semitones
                    {
                        return Err(
                            "overlapping notes require the same pitch bend on their lane channel",
                        );
                    }
                    if !self.start_note(
                        event.at,
                        format!("input:{source_id}"),
                        key,
                        velocity,
                        channel,
                        id,
                    ) {
                        continue;
                    }
                    self.channel_offsets.insert(channel, tuning_semitones);
                    let mut entry = expression::entry_messages(
                        &settings.sources,
                        &self.expression,
                        &self.envelfo_expression,
                    );
                    if settings.bend_range.is_some() && !entry.iter().any(|d| d[0] == 0xe0) {
                        entry.insert(0, vec![0xe0, 0, 64]);
                    }
                    for data in entry {
                        let mut wire = expression::tuning_message(
                            data,
                            tuning_semitones,
                            settings.bend_range,
                        )?;
                        wire[0] += channel;
                        output.push(wire);
                    }
                    output.push(vec![0x90 + channel, key, velocity]);
                }
                OutputKind::NoteOff { id, key, .. }
                    if self.sounding.contains_key(&(channel, id)) =>
                {
                    output.push(vec![0x80 + channel, key, 0]);
                    self.end_note(event.at, 0, channel, id);
                }
                _ => {}
            }
        }
        Ok(output)
    }

    fn history_messages(
        &mut self,
        events: Vec<RealizedEvent>,
    ) -> Result<Vec<Vec<u8>>, &'static str> {
        let mut output = Vec::new();
        let channel = self.channel;
        let settings = self.channels[&channel].clone();
        let attacks = events
            .iter()
            .filter(|e| e.data[0] == 0x90 && e.data[2] > 0)
            .map(|e| {
                (
                    (e.at, e.source_note.clone()),
                    (e.data[1], e.data[2], e.tuning_semitones),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut initialized = std::collections::BTreeSet::new();
        for event in events {
            let kind = event.data[0] & 0xf0;
            let group = (event.at, event.source_note.clone());
            if attacks.contains_key(&group)
                && !initialized.contains(&group)
                && !(kind == 0x80 || kind == 0x90 && event.data[2] == 0)
            {
                let (key, velocity, tuning_semitones) = attacks[&group];
                self.start_note(
                    event.at,
                    event.source_note.clone(),
                    key,
                    velocity,
                    channel,
                    0,
                );
                initialized.insert(group);
                if self.suppressed_note {
                    continue;
                }
                self.channel_offsets.insert(channel, tuning_semitones);
                let mut entry = expression::entry_messages(
                    &settings.sources,
                    &self.expression,
                    &self.envelfo_expression,
                );
                if settings.bend_range.is_some() && !entry.iter().any(|d| d[0] == 0xe0) {
                    entry.insert(0, vec![0xe0, 0, 64]);
                }
                for data in entry {
                    let mut wire =
                        expression::tuning_message(data, tuning_semitones, settings.bend_range)?;
                    wire[0] += channel;
                    output.push(wire);
                }
            }
            if self.suppressed_note {
                continue;
            }
            if let Some(lane) = expression::lane(&event.data) {
                if settings.sources[&lane] != Source::Recorded {
                    continue;
                }
            }
            if kind == 0x80 || kind == 0x90 && event.data[2] == 0 {
                self.end_note(event.at, event.data[2], channel, 0);
            }
            let mut wire = expression::tuning_message(
                event.data,
                self.channel_offsets.get(&channel).copied().unwrap_or(0.0),
                settings.bend_range,
            )?;
            if wire[0] < 0xf0 {
                wire[0] = (wire[0] & 0xf0) | channel;
            }
            output.push(wire);
        }
        Ok(output)
    }

    fn start_note(
        &mut self,
        at: arpeg_core::Beat,
        source: String,
        key: u8,
        velocity: u8,
        channel: u8,
        id: u64,
    ) -> bool {
        if self.notes.len() + self.sounding.len() + 2 > 4096 {
            self.notes_exhausted = true;
            self.suppressed_note = true;
            return false;
        }
        let note = NoteEvent {
            channel: channel + 1,
            at,
            port: NotePort::NoteStart,
            occurrence: self.next_occurrence,
            source,
            key,
            velocity,
        };
        self.next_occurrence += 1;
        self.sounding.insert((channel, id), note.clone());
        self.notes.push(note);
        self.suppressed_note = false;
        true
    }

    fn end_note(&mut self, at: arpeg_core::Beat, velocity: u8, channel: u8, id: u64) {
        if let Some(mut note) = self.sounding.remove(&(channel, id)) {
            note.at = at;
            note.port = NotePort::NoteEnd;
            note.velocity = velocity;
            self.notes.push(note);
        }
    }

    fn elapsed(&mut self, at_us: i64) -> i64 {
        let origin = *self.origin_us.get_or_insert(at_us);
        self.clock.initialize(0);
        self.wall_us = self.wall_us.max(at_us - origin);
        self.wall_us
    }
}
