//! Exact event decisions without MIDI ports, audio, or Python.

use num_rational::Ratio;

pub type Beat = Ratio<i64>;

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
    step: Beat,
    gate: Beat,
    through: Beat,
) -> Result<Vec<Occurrence<'a>>, &'static str> {
    if step <= Beat::from_integer(0) || gate < Beat::from_integer(0) {
        return Err("step must be positive and gate nonnegative");
    }
    if through < Beat::from_integer(0) {
        return Err("render horizon must be nonnegative");
    }
    let mut occurrences = Vec::new();
    let mut previous_bank = Vec::new();
    let mut previous_key: Option<(i32, &str)> = None;
    let mut revision = 0;
    let mut at = Beat::from_integer(0);
    while at < through {
        let mut active: Vec<_> = notes
            .iter()
            .filter(|note| note.onset <= at && at < note.release)
            .collect();
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
        active.sort_unstable_by_key(|note| (note.key, note.id));
        let note = active
            .iter()
            .find(|note| previous_key.is_none_or(|key| (note.key, note.id) > key))
            .unwrap_or(&active[0]);
        previous_key = Some((note.key, note.id));

        let mut gate_end = at + step * gate;
        let mut boundaries: Vec<_> = notes
            .iter()
            .flat_map(|note| [note.onset, note.release])
            .filter(|time| at < *time && *time < gate_end)
            .collect();
        boundaries.sort_unstable();
        boundaries.dedup();
        for boundary in boundaries {
            if !notes
                .iter()
                .any(|note| note.onset <= boundary && boundary < note.release)
            {
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
