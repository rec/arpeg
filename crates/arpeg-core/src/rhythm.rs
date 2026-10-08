//! Canonical rhythm decisions on a local step clock.

use crate::Beat;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Rhythm {
    Grid {
        step: Beat,
    },
    Euclidean {
        step: Beat,
        steps: i64,
        pulses: i64,
        rotation: i64,
    },
    Pattern {
        steps: Vec<PatternStep>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatternStep {
    Hit { duration: Beat, repeats: usize },
    Rest { duration: Beat },
    Tie { duration: Beat },
}

#[derive(Clone, Copy, Debug)]
pub struct RhythmDecision {
    pub duration: Beat,
    pub repeats: usize,
    pub gate: Beat,
    pub final_gate: Beat,
}

impl PatternStep {
    fn duration(self) -> Beat {
        match self {
            Self::Hit { duration, .. } | Self::Rest { duration } | Self::Tie { duration } => {
                duration
            }
        }
    }
}

impl Rhythm {
    /// The first cell of an authored rhythm loop, independently of hits and rotation.
    pub fn starts_cycle(&self, index: i64) -> bool {
        match self {
            Self::Grid { .. } => false,
            Self::Euclidean { steps, .. } => index % steps == 0,
            Self::Pattern { steps } => index as usize % steps.len() == 0,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Grid { step } | Self::Euclidean { step, .. }
                if *step <= Beat::from_integer(0) =>
            {
                return Err("rhythm step must be positive");
            }
            Self::Pattern { steps } => {
                if steps.is_empty()
                    || steps.iter().any(|step| {
                        step.duration() <= Beat::from_integer(0)
                            || matches!(step, PatternStep::Hit { repeats: 0, .. })
                    })
                {
                    return Err("pattern requires steps with positive durations and repeat counts");
                }
            }
            _ => {}
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

    /// Positive rotation moves hits later; zero pulses produces silence.
    /// The rhythm must have passed validation before use.
    pub fn allows_step(&self, index: i64) -> bool {
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
            Self::Pattern { steps } => {
                matches!(steps[index as usize % steps.len()], PatternStep::Hit { .. })
            }
        }
    }

    pub fn decide_step(&self, index: i64, gate: Beat) -> RhythmDecision {
        match self {
            Self::Grid { step } | Self::Euclidean { step, .. } => RhythmDecision {
                duration: *step,
                repeats: usize::from(self.allows_step(index)),
                gate: *step * gate,
                final_gate: *step * gate,
            },
            Self::Pattern { steps } => {
                let position = index as usize % steps.len();
                let step = steps[position];
                let duration = step.duration();
                let repeats = if let PatternStep::Hit { repeats, .. } = step {
                    repeats
                } else {
                    0
                };
                let interval = duration / repeats.max(1) as i64;
                let attack_gate = interval * gate;
                let mut final_gate = attack_gate;
                if repeats > 0 {
                    let mut held = interval;
                    for offset in 1..steps.len() {
                        let PatternStep::Tie { duration } =
                            steps[(position + offset) % steps.len()]
                        else {
                            break;
                        };
                        final_gate = attack_gate.max(held + duration * gate);
                        held += duration;
                    }
                }
                RhythmDecision {
                    duration,
                    repeats,
                    gate: attack_gate,
                    final_gate,
                }
            }
        }
    }
}
