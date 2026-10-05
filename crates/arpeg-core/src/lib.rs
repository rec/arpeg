//! Exact event decisions without MIDI ports, audio, or Python.

use num_rational::Ratio;

pub mod capture;
pub mod chance;
pub mod gesture;
pub mod history;
pub mod live;
pub mod marked_sample;
pub mod rhythm;

pub type Beat = Ratio<i64>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Selection {
    Ascending,
    Descending,
    Played,
    ReversePlayed,
    Alternating {
        repeat_endpoints: bool,
    },
    InsideOut,
    OutsideIn,
    IndexPattern {
        indices: Vec<u64>,
        rest_outside: bool,
    },
    Shuffle {
        once: bool,
        no_repeat: bool,
        preserve: bool,
    },
    Walk(Walk),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Walk {
    pub moves: Vec<i64>,
    pub weights: Vec<u64>,
    pub start_move: bool,
    pub keep_rank: bool,
}

impl Walk {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.moves.is_empty()
            || self.moves.len() != self.weights.len()
            || self.weights.contains(&0)
            || self
                .weights
                .iter()
                .try_fold(0_u64, |total, weight| total.checked_add(*weight))
                .is_none()
        {
            return Err("walk requires positive weights matching moves with a total fitting u64");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bank {
    Held,
    LatchedReplace,
    LatchedAdd,
    LatchedToggle,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HeldNote<'a> {
    pub id: &'a str,
    pub key: i32,
    pub onset: Beat,
    pub release: Beat,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Occurrence<'a> {
    pub source_id: &'a str,
    pub trigger_id: String,
    pub bank_revision: usize,
    pub decision: usize,
    pub onset: Beat,
    pub gate_end: Beat,
}

pub fn render_held<'a>(
    notes: &'a [HeldNote<'a>],
    bank_mode: Bank,
    selection: Selection,
    rhythm: rhythm::Rhythm,
    gate: Beat,
    through: Beat,
) -> Result<Vec<Occurrence<'a>>, &'static str> {
    if matches!(selection, Selection::Walk(_)) {
        return Err("walk selection currently requires live input");
    }
    if matches!(selection, Selection::Alternating { .. }) {
        return Err("alternating selection currently requires live input");
    }
    if matches!(selection, Selection::InsideOut | Selection::OutsideIn) {
        return Err("center/edge selection currently requires live input");
    }
    if matches!(selection, Selection::IndexPattern { .. }) {
        return Err("index pattern selection currently requires live input");
    }
    if matches!(selection, Selection::Shuffle { .. }) {
        return Err("shuffle selection currently requires live input");
    }
    rhythm.validate()?;
    if matches!(rhythm, rhythm::Rhythm::Pattern { .. }) {
        return Err("pattern rhythm currently requires live input");
    }
    let step = rhythm.decide_step(0, gate).duration;
    if gate < Beat::from_integer(0) {
        return Err("gate must be nonnegative");
    }
    if through < Beat::from_integer(0) {
        return Err("render horizon must be nonnegative");
    }
    let mut occurrences = Vec::new();
    let mut previous_bank = Vec::new();
    let mut previous_key: Option<(Beat, &str)> = None;
    let mut revision = 0;
    let mut at = Beat::from_integer(0);
    while at < through {
        let mut active = bank_at(notes, bank_mode, at);
        let mut bank: Vec<_> = active.iter().map(|note| note.id).collect();
        bank.sort_unstable();
        if bank != previous_bank {
            revision += 1;
            previous_bank = bank;
        }
        if active.is_empty() {
            previous_key = None;
            at += step;
            continue;
        }
        if !rhythm.allows_step((at / step).to_integer()) {
            at += step;
            continue;
        }
        active.sort_unstable_by_key(|note| selection_key(note, &selection));
        let note = active
            .iter()
            .find(|note| previous_key.is_none_or(|key| selection_key(note, &selection) > key))
            .unwrap_or(&active[0]);
        previous_key = Some(selection_key(note, &selection));

        let mut gate_end = at + step * gate;
        let mut boundaries: Vec<_> = notes
            .iter()
            .flat_map(|note| [note.onset, note.release])
            .filter(|time| at < *time && *time < gate_end)
            .collect();
        boundaries.sort_unstable();
        boundaries.dedup();
        for boundary in boundaries {
            if bank_at(notes, bank_mode, boundary).is_empty() {
                gate_end = boundary;
                break;
            }
        }
        let decision = occurrences.len();
        occurrences.push(Occurrence {
            source_id: note.id,
            trigger_id: format!("arp-{decision}"),
            bank_revision: revision,
            decision,
            onset: at,
            gate_end,
        });
        at += step;
    }
    Ok(occurrences)
}

fn selection_key<'a>(note: &HeldNote<'a>, selection: &Selection) -> (Beat, &'a str) {
    let position = match selection {
        Selection::Ascending
        | Selection::Walk(_)
        | Selection::Alternating { .. }
        | Selection::Shuffle { .. }
        | Selection::IndexPattern { .. }
        | Selection::InsideOut
        | Selection::OutsideIn => Beat::from_integer(note.key.into()),
        Selection::Descending => -Beat::from_integer(note.key.into()),
        Selection::Played => note.onset,
        Selection::ReversePlayed => -note.onset,
    };
    (position, note.id)
}

fn bank_at<'a>(notes: &'a [HeldNote<'a>], mode: Bank, at: Beat) -> Vec<&'a HeldNote<'a>> {
    if mode == Bank::Held {
        return notes
            .iter()
            .filter(|note| note.onset <= at && at < note.release)
            .collect();
    }
    let mut entries: Vec<_> = notes.iter().filter(|note| note.onset <= at).collect();
    entries.sort_unstable_by_key(|note| (note.onset, note.id));
    let mut active: Vec<&HeldNote> = Vec::new();
    let mut index = 0;
    while index < entries.len() {
        let start = entries[index].onset;
        let end = entries[index..]
            .iter()
            .position(|note| note.onset != start)
            .map_or(entries.len(), |offset| index + offset);
        let group = &entries[index..end];
        match mode {
            Bank::Held => unreachable!(),
            Bank::LatchedReplace => active = group.to_vec(),
            Bank::LatchedAdd => active.extend_from_slice(group),
            Bank::LatchedToggle => {
                let mut pitches: Vec<_> = group.iter().map(|note| note.key).collect();
                pitches.sort_unstable();
                pitches.dedup();
                for pitch in pitches {
                    if active.iter().any(|note| note.key == pitch) {
                        active.retain(|note| note.key != pitch);
                    } else {
                        active.extend(group.iter().copied().filter(|note| note.key == pitch));
                    }
                }
            }
        }
        index = end;
    }
    active
}
