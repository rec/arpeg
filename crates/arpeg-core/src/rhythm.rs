//! Canonical rhythm masks on a local step clock.

use crate::Beat;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
}

impl Rhythm {
    pub fn step(self) -> Beat {
        match self {
            Self::Grid { step } | Self::Euclidean { step, .. } => step,
        }
    }

    pub fn validate(self) -> Result<(), &'static str> {
        if self.step() <= Beat::from_integer(0) {
            return Err("rhythm step must be positive");
        }
        if let Self::Euclidean { steps, pulses, .. } = self {
            if steps <= 0 || pulses < 0 || pulses > steps {
                return Err(
                    "Euclidean rhythm requires positive steps and pulses between zero and steps",
                );
            }
        }
        Ok(())
    }

    /// Positive rotation moves hits later; zero pulses produces silence.
    /// The rhythm must have passed validation before use.
    pub fn allows_step(self, index: i64) -> bool {
        match self {
            Self::Grid { .. } => true,
            Self::Euclidean {
                steps,
                pulses,
                rotation,
                ..
            } => {
                let steps = i128::from(steps);
                let pulses = i128::from(pulses);
                let phase = (i128::from(index) - i128::from(rotation)).rem_euclid(steps);
                (phase * pulses) % steps < pulses
            }
        }
    }
}
