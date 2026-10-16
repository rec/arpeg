//! Canonical rhythm decisions on a local step clock.

use crate::Beat;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Rhythm {
    Grid {
        step: Beat,
        swing: Beat,
        groove: Option<Vec<Beat>>,
    },
    Euclidean {
        step: Beat,
        swing: Beat,
        groove: Option<Vec<Beat>>,
        steps: i64,
        pulses: i64,
        rotation: i64,
        cycles: Option<i64>,
    },
    Pattern {
        steps: Vec<PatternStep>,
        cycles: Option<i64>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PatternStep {
    Hit {
        duration: Beat,
        repeats: usize,
        condition: Option<PassCondition>,
    },
    Rest {
        duration: Beat,
    },
    Tie {
        duration: Beat,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PassCondition {
    pub passes: Vec<i64>,
    pub out_of: i64,
}

impl PassCondition {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.out_of <= 0
            || self.passes.is_empty()
            || self.passes.iter().any(|p| *p <= 0 || *p > self.out_of)
            || self
                .passes
                .iter()
                .enumerate()
                .any(|(i, p)| self.passes[..i].contains(p))
        {
            return Err("condition requires unique passes between 1 and out_of");
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct StepPatterns {
    pub accents: Vec<Beat>,
    pub gates: Vec<Beat>,
}

impl Default for StepPatterns {
    fn default() -> Self {
        Self {
            accents: vec![Beat::from_integer(1)],
            gates: vec![Beat::from_integer(1)],
        }
    }
}

impl StepPatterns {
    pub fn validate(&self) -> Result<(), &'static str> {
        if [&self.accents, &self.gates]
            .iter()
            .any(|p| p.is_empty() || p.iter().any(|v| *v < Beat::from_integer(0)))
        {
            return Err("step patterns require nonempty nonnegative exact multipliers");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RhythmDecision {
    pub duration: Beat,
    pub repeats: usize,
    pub gate: Beat,
    pub final_gate: Beat,
    pub accent: Beat,
}

impl PatternStep {
    fn duration(&self) -> Beat {
        match self {
            Self::Hit { duration, .. } | Self::Rest { duration } | Self::Tie { duration } => {
                *duration
            }
        }
    }
}

impl Rhythm {
    /// The first cell of an authored rhythm loop, independently of hits and rotation.
    pub fn starts_cycle(&self, index: i64) -> bool {
        if !self.within_cycles(index) {
            return false;
        }
        match self {
            Self::Grid { .. } => false,
            Self::Euclidean { steps, .. } => index % steps == 0,
            Self::Pattern { steps, .. } => index as usize % steps.len() == 0,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if matches!(self, Self::Euclidean { cycles: Some(c), .. } | Self::Pattern { cycles: Some(c), .. } if *c <= 0)
        {
            return Err("rhythm cycles must be positive");
        }
        match self {
            Self::Grid { step, .. } | Self::Euclidean { step, .. }
                if *step <= Beat::from_integer(0) =>
            {
                return Err("rhythm step must be positive");
            }
            Self::Pattern { steps, .. } => {
                if steps.is_empty()
                    || steps.iter().any(|step| {
                        step.duration() <= Beat::from_integer(0)
                            || matches!(step, PatternStep::Hit { repeats: 0, .. })
                    })
                {
                    return Err("pattern requires steps with positive durations and repeat counts");
                }
                for step in steps {
                    if let PatternStep::Hit {
                        condition: Some(condition),
                        ..
                    } = step
                    {
                        condition.validate()?;
                    }
                }
            }
            _ => {}
        }
        if let Self::Grid { swing, groove, .. } | Self::Euclidean { swing, groove, .. } = self {
            if *swing < Beat::new(1, 2) || *swing >= Beat::from_integer(1) {
                return Err("swing must be at least 1/2 and less than 1");
            }
            if let Some(weights) = groove {
                if weights.is_empty() || weights.iter().any(|w| *w <= Beat::from_integer(0)) {
                    return Err("groove requires nonempty positive exact weights");
                }
                if *swing != Beat::new(1, 2) {
                    return Err("groove cannot be combined with non-straight swing");
                }
            }
        }
        if let Self::Euclidean { steps, pulses, .. } = self {
            if *steps <= 0 || *pulses < 0 || pulses > steps {
                return Err(
                    "Euclidean rhythm requires positive steps and pulses between zero and steps",
                );
            }
        }
        Ok(())
    }

    /// Finite loops consume their allowance on the absolute rhythm clock.
    pub fn within_cycles(&self, index: i64) -> bool {
        match self {
            Self::Euclidean {
                steps,
                cycles: Some(cycles),
                ..
            } => index / steps < *cycles,
            Self::Pattern {
                steps,
                cycles: Some(cycles),
            } => index / (steps.len() as i64) < *cycles,
            _ => true,
        }
    }

    /// Positive rotation moves hits later; zero pulses produces silence.
    /// The rhythm must have passed validation before use.
    pub fn allows_step(&self, index: i64) -> bool {
        if !self.within_cycles(index) {
            return false;
        }
        match self {
            Self::Grid { .. } => true,
            Self::Euclidean {
                steps,
                pulses,
                rotation,
                ..
            } => {
                let steps = i128::from(*steps);
                let pulses = i128::from(*pulses);
                let phase = (i128::from(index) - i128::from(*rotation)).rem_euclid(steps);
                (phase * pulses) % steps < pulses
            }
            Self::Pattern { steps, .. } => match &steps[index as usize % steps.len()] {
                PatternStep::Hit {
                    condition: Some(condition),
                    ..
                } => condition
                    .passes
                    .contains(&(index / steps.len() as i64 % condition.out_of + 1)),
                PatternStep::Hit { .. } => true,
                _ => false,
            },
        }
    }

    pub fn decide_step(&self, index: i64, gate: Beat, patterns: &StepPatterns) -> RhythmDecision {
        let base_gate = gate;
        let gate = gate * patterns.gates[index as usize % patterns.gates.len()];
        let accent = patterns.accents[index as usize % patterns.accents.len()];
        match self {
            Self::Grid {
                step,
                swing,
                groove,
            }
            | Self::Euclidean {
                step,
                swing,
                groove,
                ..
            } => {
                let duration = grid_duration(*step, *swing, index, groove.as_deref());
                RhythmDecision {
                    accent,
                    duration,
                    repeats: usize::from(self.allows_step(index)),
                    gate: duration * gate,
                    final_gate: duration * gate,
                }
            }
            Self::Pattern { steps, .. } => {
                let position = index as usize % steps.len();
                let step = &steps[position];
                let duration = step.duration();
                let repeats = if let PatternStep::Hit { repeats, .. } = step {
                    if self.allows_step(index) { *repeats } else { 0 }
                } else {
                    0
                };
                let interval = duration / repeats.max(1) as i64;
                let attack_gate = interval * gate;
                let mut final_gate = attack_gate;
                if repeats > 0 {
                    let mut held = interval;
                    for offset in 1..steps.len() {
                        if !self.within_cycles(index + offset as i64) {
                            break;
                        }
                        let PatternStep::Tie { duration } =
                            &steps[(position + offset) % steps.len()]
                        else {
                            break;
                        };
                        final_gate = final_gate.max(
                            held + *duration
                                * base_gate
                                * patterns.gates[(index as usize + offset) % patterns.gates.len()],
                        );
                        held += *duration;
                    }
                }
                RhythmDecision {
                    accent,
                    duration,
                    repeats,
                    gate: attack_gate,
                    final_gate,
                }
            }
        }
    }
}

/// Duration of a swung interval, independent of rhythm mask and loop length.
pub fn grid_duration(step: Beat, swing: Beat, index: i64, groove: Option<&[Beat]>) -> Beat {
    if let Some(weights) = groove {
        let total: Beat = weights.iter().copied().sum();
        return step * weights.len() as i64 * weights[index as usize % weights.len()] / total;
    }
    step * 2
        * if index % 2 == 0 {
            swing
        } else {
            Beat::from_integer(1) - swing
        }
}

pub fn grid_onset(step: Beat, swing: Beat, index: i64, groove: Option<&[Beat]>) -> Beat {
    if let Some(weights) = groove {
        let length = weights.len() as i64;
        let cycles = index / length;
        let position = index as usize % weights.len();
        let total: Beat = weights.iter().copied().sum();
        let prefix: Beat = weights[..position].iter().copied().sum();
        return step * length * (Beat::from_integer(cycles) + prefix / total);
    }
    step * 2
        * (Beat::from_integer(index.div_euclid(2))
            + if index.rem_euclid(2) == 0 {
                Beat::from_integer(0)
            } else {
                swing
            })
}

/// First opportunity at or after a beat, retaining absolute pair phase.
pub fn grid_position(step: Beat, swing: Beat, at: Beat, groove: Option<&[Beat]>) -> (i64, Beat) {
    if let Some(weights) = groove {
        let length = weights.len() as i64;
        let period = step * length;
        let cycles = (at / period).floor().to_integer();
        let mut onset = period * cycles;
        let total: Beat = weights.iter().copied().sum();
        let scale = period / total;
        for (position, weight) in weights.iter().enumerate() {
            if onset >= at {
                return (cycles * length + position as i64, onset);
            }
            onset += weight * scale;
        }
        return ((cycles + 1) * length, onset);
    }
    let pair = step * 2;
    let pairs = (at / pair).floor().to_integer();
    if at == pair * pairs {
        return (2 * pairs, at);
    }
    let middle = grid_onset(step, swing, 2 * pairs + 1, None);
    if at <= middle {
        (2 * pairs + 1, middle)
    } else {
        (2 * pairs + 2, pair * (pairs + 1))
    }
}
