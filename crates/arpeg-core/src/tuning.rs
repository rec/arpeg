//! Prepared tuning coordinates, retaining fractional pitch until MIDI encoding.

pub const TUNING_ERROR: &str = "tuning cannot be realized within MIDI destination limits";

#[derive(Clone, Debug)]
pub struct PreparedTuning {
    pub pitches: Vec<f64>,
    pub first_note: i64,
    pub period: Option<i64>,
    pub period_semitones: f64,
    pub computed_limit: Option<u64>,
}

impl PreparedTuning {
    pub fn pitch(&self, index: i128) -> Result<f64, &'static str> {
        let index = index - i128::from(self.first_note);
        if let Some(limit) = self.computed_limit {
            let semitones = index as f64 * self.period_semitones / self.period.unwrap() as f64;
            let semitones = if limit == 0 {
                semitones
            } else {
                let ratio = 2.0_f64.powf(semitones / 12.0);
                let value = approximate(ratio, limit);
                if !value.is_finite() || value <= 0.0 {
                    return Err(TUNING_ERROR);
                }
                12.0 * value.log2()
            };
            return Ok(self.pitches[0] + semitones);
        }
        match self.period {
            Some(period) => Ok(self.pitches[index.rem_euclid(i128::from(period)) as usize]
                + index.div_euclid(i128::from(period)) as f64 * self.period_semitones),
            None => self
                .pitches
                .get(usize::try_from(index).map_err(|_| TUNING_ERROR)?)
                .copied()
                .ok_or(TUNING_ERROR),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RealizedPitch {
    pub key: u8,
    pub tuning_semitones: f64,
}

fn approximate(value: f64, limit: u64) -> f64 {
    if !value.is_finite() || value <= 0.0 {
        return value;
    }
    let (mut p0, mut q0, mut p1, mut q1) = (0.0, 1.0, 1.0, 0.0);
    let mut x = value;
    for _ in 0..128 {
        let a = x.floor();
        let q2 = q0 + a * q1;
        if q2 > limit as f64 {
            break;
        }
        (p0, p1) = (p1, p0 + a * p1);
        (q0, q1) = (q1, q2);
        if x == a {
            return p1 / q1;
        }
        x = 1.0 / (x - a);
    }
    let k = ((limit as f64 - q0) / q1).floor();
    let first = (p0 + k * p1) / (q0 + k * q1);
    let second = p1 / q1;
    if (second - value).abs() <= (first - value).abs() {
        second
    } else {
        first
    }
}
