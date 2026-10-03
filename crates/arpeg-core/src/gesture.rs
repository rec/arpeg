//! Place completed MIDI gestures while keeping their expression independent.

use num_rational::Ratio;

use crate::capture::{CapturedPhrase, MidiEvent, SourceNote};

pub type Tick = Ratio<i64>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Placement {
    pub note_id: String,
    pub onset: Tick,
    pub gate: Option<Tick>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RealizedEvent {
    pub at: Tick,
    pub data: Vec<u8>,
    pub source_note: String,
    pub source_event: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Timing {
    Original,
    Fit,
}

#[derive(Clone, Copy)]
struct Reservation {
    channel: u8,
    gate_end: Tick,
    last_control: Tick,
}

struct OrderedEvent {
    phase: u8,
    serial: usize,
    event: RealizedEvent,
}

pub fn render(
    phrase: &CapturedPhrase,
    placements: &[Placement],
    channels: &[u8],
    timing: Timing,
) -> Result<Vec<RealizedEvent>, &'static str> {
    if channels.is_empty()
        || channels.iter().any(|channel| *channel > 15)
        || channels
            .iter()
            .enumerate()
            .any(|(index, channel)| channels[index + 1..].contains(channel))
    {
        return Err("MIDI channels must be unique values from 0 to 15");
    }
    if placements
        .iter()
        .any(|placement| placement.onset < Tick::from_integer(0))
        || placements
            .windows(2)
            .any(|pair| pair[1].onset < pair[0].onset)
    {
        return Err("placements must have nonnegative ordered onsets");
    }
    let mut reservations = Vec::<Reservation>::new();
    let mut events = Vec::<OrderedEvent>::new();
    for placement in placements {
        let note = phrase
            .notes
            .iter()
            .find(|note| note.note_id == placement.note_id)
            .ok_or("placement references an unknown source note")?;
        let source_gate = Tick::from_integer(note.gate_end_tick - note.onset_tick);
        let output_gate = placement.gate.unwrap_or(source_gate);
        if output_gate < Tick::from_integer(0) {
            return Err("output gate must be nonnegative");
        }
        if timing == Timing::Original && output_gate != source_gate {
            return Err("original timing requires the source gate");
        }
        let scale = if timing == Timing::Fit && source_gate > Tick::from_integer(0) {
            output_gate / source_gate
        } else {
            Tick::from_integer(1)
        };
        let control_times: Vec<_> = note
            .expression_events
            .iter()
            .map(|index| {
                placement.onset
                    + Tick::from_integer(phrase.events[*index].tick - note.onset_tick) * scale
            })
            .collect();
        let channel = *channels
            .iter()
            .find(|channel| {
                reservations.iter().all(|reservation| {
                    reservation.channel != **channel
                        || reservation.gate_end <= placement.onset
                            && reservation.last_control < placement.onset
                })
            })
            .ok_or("no MIDI channel is free for independent expression")?;
        reservations.push(Reservation {
            channel,
            gate_end: placement.onset + output_gate,
            last_control: control_times
                .iter()
                .copied()
                .max()
                .unwrap_or(Tick::from_integer(-1)),
        });
        let emitter = Emitter {
            phrase,
            note,
            channel,
        };
        let mut states: Vec<_> = note.entry_state.values().collect();
        states.sort_by_key(|state| state.source_event);
        for state in states {
            if let Some(index) = state.source_event {
                emitter.append(&mut events, placement.onset, 1, Some(index), None)?;
            }
        }
        emitter.append(
            &mut events,
            placement.onset,
            2,
            Some(note.onset_event),
            None,
        )?;
        for (index, at) in note.expression_events.iter().zip(control_times) {
            emitter.append(&mut events, at, 3, Some(*index), None)?;
        }
        let release = [0x80 | channel, note.key, 0];
        emitter.append(
            &mut events,
            placement.onset + output_gate,
            0,
            note.release_event,
            Some(&release),
        )?;
    }
    events.sort_by_key(|ordered| (ordered.event.at, ordered.phase, ordered.serial));
    Ok(events.into_iter().map(|ordered| ordered.event).collect())
}

pub fn reorder(
    phrase: &CapturedPhrase,
    note_ids: &[&str],
    channels: &[u8],
) -> Result<Vec<RealizedEvent>, &'static str> {
    let mut placements = Vec::new();
    let mut at = Tick::from_integer(0);
    for note_id in note_ids {
        let note = phrase
            .notes
            .iter()
            .find(|note| note.note_id == *note_id)
            .ok_or("placement references an unknown source note")?;
        placements.push(Placement {
            note_id: (*note_id).into(),
            onset: at,
            gate: None,
        });
        at += note.cell_end_tick - note.onset_tick;
    }
    render(phrase, &placements, channels, Timing::Original)
}

struct Emitter<'a> {
    phrase: &'a CapturedPhrase,
    note: &'a SourceNote,
    channel: u8,
}

impl Emitter<'_> {
    fn append(
        &self,
        events: &mut Vec<OrderedEvent>,
        at: Tick,
        phase: u8,
        source_event: Option<usize>,
        fallback: Option<&[u8]>,
    ) -> Result<(), &'static str> {
        let data = if let Some(index) = source_event {
            midi(&self.phrase.events[index])?
        } else {
            fallback.ok_or("MIDI gesture requires source event")?
        };
        let mut data = data.to_vec();
        data[0] = data[0] & 0xf0 | self.channel;
        events.push(OrderedEvent {
            phase,
            serial: events.len(),
            event: RealizedEvent {
                at,
                data,
                source_note: self.note.note_id.clone(),
                source_event,
            },
        });
        Ok(())
    }
}

fn midi(event: &MidiEvent) -> Result<&[u8], &'static str> {
    if event.data.len() != 3 || !(0x80..0xf0).contains(&event.data[0]) {
        return Err("MIDI gesture references a non-channel event");
    }
    Ok(&event.data)
}
