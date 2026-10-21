//! Reproducible, independently named random choices without shared RNG state.

use crate::{Beat, IntervalEnvelope, Transition};

#[derive(Clone, Debug)]
pub struct Chance {
    pub probability: Beat,
    pub seed: Option<i64>,
    pub name: String,
}

impl Default for Chance {
    fn default() -> Self {
        Self {
            probability: Beat::from_integer(1),
            seed: None,
            name: String::new(),
        }
    }
}

impl Chance {
    pub fn transition_rank(
        &self,
        selection: &Transition,
        size: usize,
        previous: Option<usize>,
        revision: u64,
        decision: &mut u64,
    ) -> Option<usize> {
        if previous.is_none() && !selection.start_random {
            return Some(0);
        }
        let row = previous.and_then(|i| selection.weights.get(i));
        let mut weights: Vec<_> = (0..size)
            .map(|i| {
                u64::from(if previous.is_none() {
                    1
                } else {
                    row.and_then(|r| r.get(i))
                        .copied()
                        .unwrap_or(selection.missing_weight)
                })
            })
            .collect();
        if weights.iter().all(|w| *w == 0) {
            if !selection.uniform_when_blocked {
                return None;
            }
            weights.fill(1);
        }
        let mut chosen = draw_below(
            self.seed.expect("validated transition seed"),
            &self.name,
            "transition",
            revision,
            *decision,
            weights.iter().sum(),
        );
        *decision += 1;
        for (i, weight) in weights.into_iter().enumerate() {
            if chosen < weight {
                return Some(i);
            }
            chosen -= weight;
        }
        unreachable!("draw is below the total weight")
    }

    pub fn interval_rank(
        &self,
        selection: &IntervalEnvelope,
        size: usize,
        previous: Option<usize>,
        revision: u64,
        step: i64,
    ) -> Option<usize> {
        let Some(previous) = previous else {
            return Some(0);
        };
        let range = selection.ranges[step as usize % selection.ranges.len()];
        let (mut low, mut high) = (i128::from(range.min), i128::from(range.max));
        if selection.rest_outside {
            low = low.max(-(previous as i128));
            high = high.min(size as i128 - 1 - previous as i128);
            if low > high {
                return None;
            }
        }
        let movement = low
            + i128::from(draw_below(
                self.seed.expect("validated envelope seed"),
                &self.name,
                "interval_envelope",
                revision,
                step as u64,
                (high - low + 1) as u64,
            ));
        Some((previous as i128 + movement).rem_euclid(size as i128) as usize)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.probability < Beat::from_integer(0) || self.probability > Beat::from_integer(1) {
            return Err("probability must be between zero and one");
        }
        if self.probability > Beat::from_integer(0)
            && self.probability < Beat::from_integer(1)
            && self.seed.is_none()
        {
            return Err("probability between zero and one requires an explicit seed");
        }
        Ok(())
    }

    pub fn allows(&self, revision: u64, decision: u64, probability: Beat) -> bool {
        if probability == Beat::from_integer(0) {
            return false;
        }
        if probability == Beat::from_integer(1) {
            return true;
        }
        draw_below(
            self.seed.expect("validated seed"),
            &self.name,
            "probability",
            revision,
            decision,
            *probability.denom() as u64,
        ) < *probability.numer() as u64
    }
}

pub fn draw_below(
    seed: i64,
    name: &str,
    lane: &str,
    revision: u64,
    decision: u64,
    bound: u64,
) -> u64 {
    let bits = 64 - (bound - 1).leading_zeros();
    if bits == 0 {
        return 0;
    }
    let mask = u64::MAX >> (64 - bits);
    let mut retry = 0;
    loop {
        let text = format!(
            "arpeg-v1:{seed}:{}:{name}:{lane}:{revision}:{decision}:{retry}:0",
            name.len()
        );
        let mut word = 0xcbf29ce484222325_u64;
        for byte in text.bytes() {
            word = (word ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        word = (word ^ (word >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        word = (word ^ (word >> 27)).wrapping_mul(0x94d049bb133111eb);
        let value = (word ^ (word >> 31)) & mask;
        if value < bound {
            return value;
        }
        retry += 1;
    }
}
