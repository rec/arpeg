//! Reproducible, independently named random choices without shared RNG state.

use crate::Beat;

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
