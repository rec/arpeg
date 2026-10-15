//! Payload-independent bank, selection, rhythm, and occurrence lifecycle.
use std::cmp::Reverse;

use crate::chance::{Chance, draw_below};
use crate::timing_ports::{InputPort, TimingPorts};
use crate::{
    Bank, Beat, Selection,
    rhythm::{PatternStep, Rhythm, RhythmDecision},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Retrigger {
    OnEmpty,
    BankEdit,
}

#[derive(Clone, Copy, Debug)]
pub struct Item<V: Copy> {
    pub id: u64,
    pub source_id: u64,
    pub identity: u64,
    pub rank: i128,
    pub layer: usize,
    pub onset: Beat,
    pub value: V,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SelectionPosition {
    Onset(Beat),
    Rank(i128),
    ReverseRank(Reverse<i128>),
}

pub trait Processor {
    type Value: Copy;
    type Event;
    fn timing(&self) -> &TimingPorts;
    fn timing_mut(&mut self) -> &mut TimingPorts;
    fn expand(&self, active: &[Item<Self::Value>]) -> Result<Vec<Item<Self::Value>>, &'static str> {
        Ok(active.to_vec())
    }
    fn realize(&self, item: &Item<Self::Value>) -> Result<Option<Self::Value>, &'static str> {
        Ok(Some(item.value))
    }
    fn conflicts(&self, _left: Self::Value, _right: Self::Value) -> bool {
        false
    }
    fn event(
        &self,
        at: Beat,
        start: bool,
        id: u64,
        source_id: u64,
        value: Self::Value,
    ) -> Self::Event;
    fn entry_key(
        &self,
        item: &Item<Self::Value>,
        position: SelectionPosition,
        _reverse: bool,
    ) -> (i64, SelectionPosition, u64, i64, u64) {
        (0, position, item.source_id, 0, item.id)
    }
    fn check_control(
        &self,
        port: InputPort,
        value: Beat,
        seed: Option<i64>,
    ) -> Result<(), &'static str> {
        TimingPorts::check_control(port, value, seed)
    }
    fn queue(&mut self, port: InputPort, value: Beat) -> Result<(), &'static str> {
        self.timing_mut().queue(port, value)
    }
    fn cancel_pending(&mut self) {
        self.timing_mut().cancel_pending();
    }
    fn begin_step(&mut self, at: Beat, index: i64, revision: u64, cycle: bool) -> bool {
        self.timing_mut()
            .begin_step(at, index, revision, false, cycle)
    }
}

#[derive(Clone, Copy, Debug)]
struct Active<V: Copy> {
    id: u64,
    source_id: u64,
    value: V,
    end: Beat,
}
#[derive(Clone, Copy, Debug)]
struct Attack<V: Copy> {
    at: Beat,
    note: Item<V>,
    value: V,
    gate: Beat,
}

pub struct Arpeggiator<P: Processor> {
    bank_mode: Bank,
    pub chance: Chance,
    bank_revision: u64,
    decision_count: u64,
    walk_count: u64,
    walk_rank: usize,
    pattern_position: usize,
    shuffle_order: Vec<u64>,
    shuffle_position: usize,
    shuffle_revision: Option<u64>,
    shuffle_count: u64,
    choice_count: u64,
    rising: bool,
    retrigger: Retrigger,
    selection: Selection,
    rhythm: Rhythm,
    pub ports: P,
    next_step: Beat,
    step_index: i64,
    pending: Vec<Attack<P::Value>>,
    now: Beat,
    next_id: u64,
    next_output: u64,
    previous_key: Option<(i64, SelectionPosition, u64, i64, u64)>,
    input: Vec<Item<P::Value>>,
    bank: Vec<Item<P::Value>>,
    toggle_at: Option<Beat>,
    toggled_keys: Vec<u64>,
    toggle_added_keys: Vec<u64>,
    sounding: Vec<Active<P::Value>>,
}

impl<P: Processor> Arpeggiator<P> {
    pub fn new(
        bank: Bank,
        selection: Selection,
        rhythm: Rhythm,
        ports: P,
        retrigger: Retrigger,
        chance: Chance,
    ) -> Result<Self, &'static str> {
        rhythm.validate()?;
        chance.validate()?;
        if let Selection::Choice { weights, .. } = &selection {
            if weights.is_empty() || weights.contains(&0) {
                return Err("choice requires positive weights");
            }
            if chance.seed.is_none() {
                return Err("choice requires an explicit seed");
            }
        }
        if matches!(selection, Selection::Shuffle { .. }) && chance.seed.is_none() {
            return Err("shuffle requires an explicit seed");
        }
        if let Selection::IndexPattern { indices, .. } = &selection {
            if indices.is_empty() {
                return Err("index pattern requires at least one index");
            }
        }
        if let Selection::Walk(walk) = &selection {
            walk.validate()?;
            if walk.moves.len() > 1 && chance.seed.is_none() {
                return Err("weighted walk requires an explicit seed");
            }
        }
        if ports.timing().gate < Beat::from_integer(0) {
            return Err("gate must be nonnegative");
        }
        Ok(Self {
            bank_mode: bank,
            ports,
            chance,
            bank_revision: 0,
            decision_count: 0,
            walk_count: 0,
            walk_rank: 0,
            pattern_position: 0,
            shuffle_order: Vec::new(),
            shuffle_position: 0,
            shuffle_revision: None,
            shuffle_count: 0,
            choice_count: 0,
            rising: true,
            retrigger,
            selection,
            rhythm,
            next_step: Beat::from_integer(0),
            step_index: 0,
            pending: Vec::new(),
            now: Beat::from_integer(0),
            next_id: 0,
            next_output: 0,
            previous_key: None,
            input: Vec::new(),
            bank: Vec::new(),
            toggle_at: None,
            toggled_keys: Vec::new(),
            toggle_added_keys: Vec::new(),
            sounding: Vec::new(),
        })
    }

    pub fn press(
        &mut self,
        at: Beat,
        identity: u64,
        rank: i128,
        value: P::Value,
    ) -> Result<Vec<P::Event>, &'static str> {
        self.check_time(at)?;
        let mut output = self.process_until(at, false)?;
        let new_chord = self.input.is_empty();
        let note = Item {
            id: self.next_id,
            source_id: self.next_id,
            layer: 0,
            identity,
            rank,
            value,
            onset: at,
        };
        self.input.push(note);
        let edited = match self.bank_mode {
            Bank::Held => true,
            Bank::LatchedReplace => {
                if new_chord {
                    self.bank.clear();
                }
                self.bank.push(note);
                true
            }
            Bank::LatchedAdd => {
                self.bank.push(note);
                true
            }
            Bank::LatchedToggle => {
                if self.toggle_at != Some(at) {
                    self.toggle_at = Some(at);
                    self.toggled_keys.clear();
                    self.toggle_added_keys.clear();
                }
                if !self.toggled_keys.contains(&identity) {
                    self.toggled_keys.push(identity);
                    if self.bank.iter().any(|entry| entry.identity == identity) {
                        self.bank.retain(|entry| entry.identity != identity);
                    } else {
                        self.bank.push(note);
                        self.toggle_added_keys.push(identity);
                    }
                    true
                } else if self.toggle_added_keys.contains(&identity) {
                    self.bank.push(note);
                    true
                } else {
                    false
                }
            }
        };
        if self.bank_mode != Bank::Held && self.bank.is_empty() {
            self.previous_key = None;
            self.pattern_position = 0;
            self.shuffle_order.clear();
            self.shuffle_position = 0;
            output.extend(self.release_all(at));
        }
        if edited {
            self.bank_revision += 1;
        }
        if edited && self.retrigger == Retrigger::BankEdit {
            self.previous_key = None;
            self.pattern_position = 0;
            self.shuffle_order.clear();
            self.shuffle_position = 0;
        }
        self.next_id += 1;
        Ok(output)
    }

    pub fn release(&mut self, at: Beat, identity: u64) -> Result<Vec<P::Event>, &'static str> {
        self.check_time(at)?;
        let index = self
            .input
            .iter()
            .position(|note| note.identity == identity)
            .ok_or("release has no matching onset")?;
        let mut output = self.process_until(at, false)?;
        self.input.remove(index);
        if self.bank_mode == Bank::Held {
            self.bank_revision += 1;
        }
        if self.bank_mode == Bank::Held && self.retrigger == Retrigger::BankEdit {
            self.previous_key = None;
            self.pattern_position = 0;
            self.shuffle_order.clear();
            self.shuffle_position = 0;
        }
        if self.bank_mode == Bank::Held && self.input.is_empty() {
            self.previous_key = None;
            self.pattern_position = 0;
            self.shuffle_order.clear();
            self.shuffle_position = 0;
            output.extend(self.release_all(at));
        }
        Ok(output)
    }

    pub fn advance(&mut self, through: Beat) -> Result<Vec<P::Event>, &'static str> {
        self.check_time(through)?;
        self.process_until(through, true)
    }

    pub fn before(&mut self, through: Beat) -> Result<Vec<P::Event>, &'static str> {
        self.check_time(through)?;
        let mut output = self.process_until(through, false)?;
        self.release_due(through, &mut output);
        Ok(output)
    }

    pub fn control(
        &mut self,
        at: Beat,
        port: InputPort,
        value: Beat,
    ) -> Result<Vec<P::Event>, &'static str> {
        self.check_time(at)?;
        self.ports.check_control(port, value, self.chance.seed)?;
        let output = self.before(at)?;
        self.ports.queue(port, value)?;
        Ok(output)
    }

    pub fn pause(&mut self, at: Beat) -> Vec<P::Event> {
        let output = self.release_all(at);
        self.now = at;
        while self.next_step < at {
            self.next_step += self
                .rhythm
                .decide_step(self.step_index, self.ports.timing().gate)
                .duration;
            self.step_index += 1;
        }
        output
    }

    pub fn relocate(&mut self, at: Beat) -> Vec<P::Event> {
        let output = self.release_all(at);
        self.ports.cancel_pending();
        match &self.rhythm {
            Rhythm::Grid { step } | Rhythm::Euclidean { step, .. } => {
                self.step_index = (at / step).ceil().to_integer();
                self.next_step = *step * self.step_index;
            }
            Rhythm::Pattern { steps } => {
                let cycle: Beat = (0..steps.len())
                    .map(|i| {
                        self.rhythm
                            .decide_step(i as i64, self.ports.timing().gate)
                            .duration
                    })
                    .sum();
                let cycles = (at / cycle).floor().to_integer();
                self.next_step = cycle * cycles;
                self.step_index = cycles * steps.len() as i64;
                while self.next_step < at {
                    self.next_step += self
                        .rhythm
                        .decide_step(self.step_index, self.ports.timing().gate)
                        .duration;
                    self.step_index += 1;
                }
            }
        }
        self.now = at;
        self.previous_key = None;
        self.rising = true;
        self.pattern_position = 0;
        self.shuffle_order.clear();
        self.shuffle_position = 0;
        output
    }

    pub fn stop(&mut self, at: Beat) -> Result<Vec<P::Event>, &'static str> {
        self.check_time(at)?;
        let mut output = self.process_until(at, false)?;
        output.extend(self.release_all(at));
        if !(if self.bank_mode == Bank::Held {
            &self.input
        } else {
            &self.bank
        })
        .is_empty()
        {
            self.bank_revision += 1;
        }
        self.input.clear();
        self.bank.clear();
        self.toggle_at = None;
        self.toggled_keys.clear();
        self.toggle_added_keys.clear();
        self.previous_key = None;
        self.pattern_position = 0;
        self.shuffle_order.clear();
        self.shuffle_position = 0;
        self.now = at;
        Ok(output)
    }

    pub fn clear(&mut self, at: Beat) -> Result<Vec<P::Event>, &'static str> {
        self.check_time(at)?;
        if self.bank_mode == Bank::Held {
            return Err("clear requires a latched bank");
        }
        let mut output = self.process_until(at, false)?;
        if !self.bank.is_empty() {
            self.bank_revision += 1;
        }
        self.bank.clear();
        self.toggle_at = None;
        self.toggled_keys.clear();
        self.toggle_added_keys.clear();
        self.previous_key = None;
        self.pattern_position = 0;
        self.shuffle_order.clear();
        self.shuffle_position = 0;
        output.extend(self.release_all(at));
        Ok(output)
    }

    pub fn next_deadline(&self) -> Beat {
        self.sounding
            .iter()
            .map(|note| note.end)
            .chain(self.pending.iter().map(|attack| attack.at))
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

    fn process_until(
        &mut self,
        through: Beat,
        inclusive: bool,
    ) -> Result<Vec<P::Event>, &'static str> {
        let mut output = Vec::new();
        loop {
            let next = self.next_deadline();
            if next > through || (!inclusive && next == through) {
                break;
            }
            self.release_due(next, &mut output);
            if self.next_step == next {
                let admitted = self.ports.begin_step(
                    next,
                    self.step_index,
                    self.bank_revision,
                    self.rhythm.starts_cycle(self.step_index),
                );
                let decision = self
                    .rhythm
                    .decide_step(self.step_index, self.ports.timing().gate);
                if admitted {
                    let hit = self.schedule_step(next, decision)?;
                    if !matches!(&self.rhythm, Rhythm::Pattern { steps } if matches!(steps[self.step_index as usize % steps.len()], PatternStep::Tie { .. }))
                    {
                        self.ports.timing_mut().outcome(hit);
                    }
                }
                self.next_step += decision.duration;
                self.step_index += 1;
            }
            while self.pending.first().is_some_and(|attack| attack.at == next) {
                let attack = self.pending.remove(0);
                self.attack(attack, &mut output);
            }
        }
        self.now = through;
        Ok(output)
    }

    fn release_due(&mut self, at: Beat, output: &mut Vec<P::Event>) {
        self.sounding.retain(|note| {
            if note.end <= at {
                output.push(
                    self.ports
                        .event(note.end, false, note.id, note.source_id, note.value),
                );
                false
            } else {
                true
            }
        });
    }

    fn release_all(&mut self, at: Beat) -> Vec<P::Event> {
        self.pending.clear();
        self.sounding
            .drain(..)
            .map(|note| {
                self.ports
                    .event(at, false, note.id, note.source_id, note.value)
            })
            .collect()
    }

    fn schedule_step(&mut self, at: Beat, decision: RhythmDecision) -> Result<bool, &'static str> {
        let active = if self.bank_mode == Bank::Held {
            &self.input
        } else {
            &self.bank
        };
        let active = self.ports.expand(active)?;
        if active.is_empty() {
            self.previous_key = None;
            self.pattern_position = 0;
            self.shuffle_order.clear();
            self.shuffle_position = 0;
            return Ok(false);
        }
        if decision.repeats == 0 {
            return Ok(false);
        }
        let chance_index = self.decision_count;
        self.decision_count += 1;
        if !self.chance.allows(
            self.bank_revision,
            chance_index,
            self.ports.timing().density,
        ) {
            return Ok(false);
        }
        let selection = &self.selection;
        let mut ordered = active.clone();
        ordered.sort_unstable_by_key(|note| self.selection_key(note, selection));
        let selected = if let Selection::IndexPattern {
            indices,
            rest_outside,
        } = selection
        {
            let index = indices[self.pattern_position];
            self.pattern_position = (self.pattern_position + 1) % indices.len();
            if *rest_outside && index >= ordered.len() as u64 {
                return Ok(false);
            }
            ordered[(index % ordered.len() as u64) as usize]
        } else if let Selection::Choice {
            weights,
            repeat_weights,
            no_repeat,
        } = selection
        {
            let candidates: Vec<_> = ordered
                .iter()
                .enumerate()
                .filter_map(|(i, n)| {
                    if *no_repeat
                        && ordered.len() > 1
                        && self.previous_key.is_some_and(|(_, _, _, _, id)| id == n.id)
                    {
                        return None;
                    }
                    let weight = if *repeat_weights {
                        weights[i % weights.len()]
                    } else {
                        weights.get(i).copied().unwrap_or(1)
                    };
                    Some((*n, u64::from(weight)))
                })
                .collect();
            let mut chosen = draw_below(
                self.chance.seed.expect("validated choice seed"),
                &self.chance.name,
                "choice",
                self.bank_revision,
                self.choice_count,
                candidates.iter().map(|(_, w)| w).sum(),
            );
            self.choice_count += 1;
            candidates
                .into_iter()
                .find_map(|(note, weight)| {
                    if chosen < weight {
                        Some(note)
                    } else {
                        chosen -= weight;
                        None
                    }
                })
                .expect("positive choice weights")
        } else if let Selection::Shuffle {
            once,
            no_repeat,
            preserve,
        } = selection
        {
            let seed = self.chance.seed.expect("validated shuffle seed");
            let identities: Vec<_> = ordered.iter().map(|n| n.id).collect();
            let edited = self.shuffle_revision != Some(self.bank_revision);
            if edited && !self.shuffle_order.is_empty() && *preserve {
                let played = self.shuffle_order[..self.shuffle_position].to_vec();
                self.shuffle_order.retain(|i| identities.contains(i));
                self.shuffle_position = played.iter().filter(|i| identities.contains(i)).count();
                for identity in &identities {
                    if !self.shuffle_order.contains(identity) {
                        let offset = draw_below(
                            seed,
                            &self.chance.name,
                            "shuffle",
                            self.bank_revision,
                            self.shuffle_count,
                            (self.shuffle_order.len() - self.shuffle_position + 1) as u64,
                        ) as usize;
                        self.shuffle_count += 1;
                        self.shuffle_order
                            .insert(self.shuffle_position + offset, *identity);
                    }
                }
            }
            let restart = self.shuffle_order.is_empty() || (edited && !preserve);
            let finished = self.shuffle_position == self.shuffle_order.len();
            if restart || (finished && !once) {
                self.shuffle_order = identities;
                self.shuffle_position = 0;
                for index in (1..self.shuffle_order.len()).rev() {
                    let chosen = draw_below(
                        seed,
                        &self.chance.name,
                        "shuffle",
                        self.bank_revision,
                        self.shuffle_count,
                        (index + 1) as u64,
                    ) as usize;
                    self.shuffle_count += 1;
                    self.shuffle_order.swap(index, chosen);
                }
                if *no_repeat
                    && self.shuffle_order.len() > 1
                    && self
                        .previous_key
                        .is_some_and(|(_, _, _, _, id)| self.shuffle_order[0] == id)
                {
                    let chosen = 1 + draw_below(
                        seed,
                        &self.chance.name,
                        "shuffle",
                        self.bank_revision,
                        self.shuffle_count,
                        (self.shuffle_order.len() - 1) as u64,
                    ) as usize;
                    self.shuffle_count += 1;
                    self.shuffle_order.swap(0, chosen);
                }
            } else if finished {
                self.shuffle_position = 0;
            }
            self.shuffle_revision = Some(self.bank_revision);
            let identity = self.shuffle_order[self.shuffle_position];
            self.shuffle_position += 1;
            *ordered
                .iter()
                .find(|n| n.id == identity)
                .expect("current shuffle identity")
        } else if matches!(selection, Selection::InsideOut | Selection::OutsideIn) {
            let size = ordered.len();
            let mut indices: Vec<_> = (0..size).collect();
            indices.sort_by_key(|i| {
                let distance = if matches!(selection, Selection::InsideOut) {
                    (2 * i).abs_diff(size - 1)
                } else {
                    (*i).min(size - 1 - i)
                };
                (distance, *i)
            });
            ordered = indices.into_iter().map(|i| ordered[i]).collect();
            let previous = self
                .previous_key
                .and_then(|(_, _, _, _, id)| ordered.iter().position(|note| note.id == id));
            ordered[previous.map_or(0, |i| (i + 1) % size)]
        } else if let Selection::Alternating { repeat_endpoints } = selection {
            if let Some(previous) = self.previous_key {
                if ordered.len() == 1 {
                    ordered[0]
                } else {
                    if !self.rising {
                        ordered.reverse();
                    }
                    let next = ordered.iter().find(|note| {
                        let key = self.selection_key(note, selection);
                        if self.rising {
                            key > previous
                        } else {
                            key < previous
                        }
                    });
                    if let Some(note) = next {
                        *note
                    } else {
                        self.rising = !self.rising;
                        ordered.reverse();
                        *ordered
                            .iter()
                            .find(|note| {
                                let key = self.selection_key(note, selection);
                                (if self.rising {
                                    key >= previous
                                } else {
                                    key <= previous
                                }) && (*repeat_endpoints || note.id != previous.4)
                            })
                            .unwrap_or(&ordered[0])
                    }
                }
            } else {
                self.rising = true;
                ordered[0]
            }
        } else if let Selection::Walk(walk) = selection {
            let previous = self
                .previous_key
                .and_then(|(_, _, _, _, id)| ordered.iter().position(|note| note.id == id));
            let should_move = previous.is_some()
                || if self.previous_key.is_none() {
                    walk.start_move
                } else {
                    walk.keep_rank
                };
            let mut rank = previous.unwrap_or(if self.previous_key.is_some() && walk.keep_rank {
                self.walk_rank % ordered.len()
            } else {
                0
            });
            if should_move {
                let mut chosen = draw_below(
                    self.chance.seed.unwrap_or(0),
                    &self.chance.name,
                    "walk",
                    self.bank_revision,
                    self.walk_count,
                    walk.weights.iter().sum(),
                );
                for (offset, weight) in walk.moves.iter().zip(&walk.weights) {
                    if chosen < *weight {
                        rank = (rank as i128 + i128::from(*offset))
                            .rem_euclid(ordered.len() as i128)
                            as usize;
                        break;
                    }
                    chosen -= weight;
                }
            }
            self.walk_rank = rank;
            self.walk_count += 1;
            ordered[rank]
        } else {
            *ordered
                .iter()
                .find(|note| {
                    self.previous_key
                        .is_none_or(|previous| self.selection_key(note, selection) > previous)
                })
                .unwrap_or(&ordered[0])
        };
        self.previous_key = Some(self.selection_key(&selected, selection));
        let selected = if self.ports.timing().selection_offset != 0 {
            let mut ranked = ordered.clone();
            ranked.sort_unstable_by_key(|n| self.selection_key(n, &Selection::Ascending));
            let rank = ranked.iter().position(|n| n.id == selected.id).unwrap();
            let Some(rank) = self.ports.timing().offset_rank(rank, ranked.len()) else {
                return Ok(false);
            };
            ranked[rank]
        } else {
            selected
        };
        let Some(value) = self.ports.realize(&selected)? else {
            return Ok(false);
        };
        let interval = decision.duration / decision.repeats as i64;
        self.pending.extend((0..decision.repeats).map(|i| Attack {
            at: at + interval * i as i64,
            note: selected,
            value,
            gate: if i == decision.repeats - 1 {
                decision.final_gate
            } else {
                decision.gate
            },
        }));
        Ok(true)
    }

    fn attack(&mut self, attack: Attack<P::Value>, output: &mut Vec<P::Event>) {
        let at = attack.at;
        let selected = attack.note;
        let end = at + attack.gate;
        for note in self
            .sounding
            .iter_mut()
            .filter(|n| self.ports.conflicts(n.value, attack.value))
        {
            output.push(
                self.ports
                    .event(at, false, note.id, note.source_id, note.value),
            );
            note.end = at;
        }
        self.sounding.retain(|note| note.end > at);
        let id = self.next_output;
        self.next_output += 1;
        output.push(
            self.ports
                .event(at, true, id, selected.source_id, attack.value),
        );
        if end == at {
            output.push(
                self.ports
                    .event(at, false, id, selected.source_id, attack.value),
            );
        } else {
            self.sounding.push(Active {
                id,
                source_id: selected.source_id,
                value: attack.value,
                end,
            });
        }
    }

    fn selection_key(
        &self,
        note: &Item<P::Value>,
        selection: &Selection,
    ) -> (i64, SelectionPosition, u64, i64, u64) {
        let position = match selection {
            Selection::Played => SelectionPosition::Onset(note.onset),
            Selection::ReversePlayed => SelectionPosition::Onset(-note.onset),
            Selection::Descending => SelectionPosition::ReverseRank(Reverse(note.rank)),
            _ => SelectionPosition::Rank(note.rank),
        };
        self.ports.entry_key(
            note,
            position,
            matches!(selection, Selection::Descending | Selection::ReversePlayed),
        )
    }
}
