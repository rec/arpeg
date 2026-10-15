//! Parse the shared uFor scale and tuning definitions during preparation.

use arpeg_core::{
    ports::{ScaleMapping, ScaleTransform},
    tuning::PreparedTuning,
};
use toml::{Table, Value};

pub fn scale(value: &Value) -> Result<ScaleTransform, String> {
    let table = value.as_table().ok_or("scale must be a table")?;
    fields(table, &["definition", "degrees", "mapping"])?;
    let d = table
        .get("definition")
        .and_then(Value::as_table)
        .ok_or("scale requires its shared definition")?;
    fields(
        d,
        &[
            "note_names",
            "begin",
            "end",
            "root",
            "notes",
            "intervals",
            "accidentals",
            "offset",
        ],
    )?;
    let alphabet: Vec<_> = text(d, "note_names", "ABCDEFGHIJKLMNOPQRSTUVWXYZ")?
        .chars()
        .collect();
    let position = |name: &str, default: &str| -> Result<usize, String> {
        let name = text(d, name, default)?;
        if name.chars().count() != 1 {
            return Err("scale range names must be single characters".into());
        }
        alphabet
            .iter()
            .position(|c| name.starts_with(*c))
            .ok_or("scale range name absent from note_names".into())
    };
    let begin = position("begin", "A")?;
    let root = position("root", "C")?;
    let end = position("end", "G")?;
    if begin > root || root > end {
        return Err("scale begin/root/end must be ordered".into());
    }
    let names: Vec<_> = alphabet[root..=end]
        .iter()
        .chain(alphabet[begin..root].iter())
        .copied()
        .collect();
    let steps: Vec<i64> = match d.get("intervals") {
        None => vec![2, 2, 1, 2, 2, 2, 1],
        Some(Value::String(s)) => s
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| {
                c.to_digit(10)
                    .map(i64::from)
                    .ok_or("invalid scale interval".into())
            })
            .collect::<Result<_, String>>()?,
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| {
                v.as_integer()
                    .or_else(|| {
                        v.as_str().and_then(|s| {
                            s.trim()
                                .trim_end_matches("semitones")
                                .trim_end_matches("semitone")
                                .trim()
                                .parse()
                                .ok()
                        })
                    })
                    .filter(|v| *v >= 0)
                    .ok_or("invalid scale interval".into())
            })
            .collect::<Result<_, String>>()?,
        _ => return Err("scale intervals must be a string or list".into()),
    };
    if steps.is_empty() {
        return Err("scale intervals must not be empty".into());
    }
    let accidentals = text(d, "accidentals", "whole")?;
    if !["none", "whole", "half"].contains(&accidentals) {
        return Err("invalid scale accidentals".into());
    }
    let selected = match d.get("notes") {
        None => None,
        Some(v) => {
            let text = v
                .as_str()
                .ok_or("scale notes must be text")?
                .replace('#', "♯")
                .replace('b', "♭");
            let mut tokens: Vec<String> = Vec::new();
            let mut attached = false;
            for c in text.chars() {
                if names.contains(&c) {
                    tokens.push(c.to_string());
                    attached = true;
                } else if ("♭♯".contains(c) && accidentals != "none")
                    || ("v^".contains(c) && accidentals == "half")
                {
                    if attached {
                        if let Some(token) = tokens.last_mut() {
                            token.push(c);
                        }
                    }
                } else {
                    attached = false;
                }
            }
            if tokens.is_empty() {
                Some(names.iter().map(char::to_string).collect::<Vec<_>>())
            } else {
                Some(tokens)
            }
        }
    };
    let mut intervals = Vec::new();
    let mut period = 0_i64;
    let mut position = 0_i64;
    for (i, name) in names.iter().enumerate() {
        let step = steps[i % steps.len()];
        if selected
            .as_ref()
            .is_some_and(|s| s.contains(&name.to_string()))
        {
            intervals.push(position);
        }
        position += 1;
        for j in 1..step {
            let flat = spelling(names[(i + 1) % names.len()], step - j, false, accidentals);
            let sharp = spelling(*name, j, true, accidentals);
            if selected
                .as_ref()
                .is_some_and(|s| s.contains(&flat) || s.contains(&sharp))
            {
                intervals.push(position);
            }
            position += 1;
        }
        period = period
            .checked_add(step)
            .ok_or("scale period is too large")?;
    }
    if selected.is_none() {
        intervals = (0..period).collect();
    }
    if period <= 0 || intervals.is_empty() {
        return Err("scale mapping requires a nonempty positive-period scale".into());
    }
    let mapping = match text(table, "mapping", "nearest")? {
        "nearest" => ScaleMapping::Nearest,
        "up" => ScaleMapping::Up,
        "down" => ScaleMapping::Down,
        "drop" => ScaleMapping::Drop,
        _ => return Err("invalid scale mapping".into()),
    };
    Ok(ScaleTransform {
        offset: integer(d, "offset", 0)?,
        period,
        intervals,
        degrees: integer(table, "degrees", 0)?,
        mapping,
    })
}

pub fn prepare(value: &Value) -> Result<PreparedTuning, String> {
    let t = value.as_table().ok_or("tuning must be a table")?;
    fields(
        t,
        &["source", "root_note", "root_frequency", "detune_cents"],
    )?;
    let source = t
        .get("source")
        .and_then(Value::as_table)
        .ok_or("tuning requires source")?;
    let kind = text(source, "kind", "")?;
    let detune = number(t, "detune_cents", 0.0)? / 100.0;
    let root = 69.0
        + 12.0 * (pitch_logarithm(text(t, "root_frequency", "440")?)? - 440.0_f64.log2())
        + detune;
    let mut result = PreparedTuning {
        pitches: Vec::new(),
        first_note: integer(t, "root_note", 69)?,
        period: None,
        period_semitones: 0.0,
        computed_limit: None,
    };
    match kind {
        "computed" => {
            fields(
                source,
                &[
                    "kind",
                    "notes_per_octave",
                    "octave_ratio",
                    "denominator_limit",
                ],
            )?;
            let steps = integer(source, "notes_per_octave", 12)?;
            let limit = integer(source, "denominator_limit", 0)?;
            if steps <= 0 || limit < 0 {
                return Err("invalid computed tuning bounds".into());
            }
            result.period = Some(steps);
            result.period_semitones = 12.0 * pitch_logarithm(text(source, "octave_ratio", "2")?)?;
            result.pitches = vec![root];
            result.computed_limit = Some(limit as u64);
        }
        "ratios" => {
            fields(
                source,
                &["kind", "values", "repeat_ratio", "name", "description"],
            )?;
            result.pitches = expressions(source, "values")?
                .into_iter()
                .map(|v| root + 12.0 * v)
                .collect();
            if let Some(v) = source.get("repeat_ratio") {
                result.period = Some(result.pitches.len() as i64);
                result.period_semitones =
                    12.0 * pitch_logarithm(v.as_str().ok_or("repeat_ratio must be text")?)?;
            }
        }
        "intervals" => {
            fields(source, &["kind", "intervals", "repeat"])?;
            result.pitches = vec![root];
            for v in expressions(source, "intervals")? {
                result
                    .pitches
                    .push(result.pitches.last().unwrap() + 12.0 * v);
            }
            let repeat = source
                .get("repeat")
                .map_or(Ok(true), |v| v.as_bool().ok_or("repeat must be boolean"))?;
            if repeat {
                result.period = Some(result.pitches.len() as i64 - 1);
                result.period_semitones = result.pitches.pop().unwrap() - root;
            }
        }
        "frequencies" => {
            fields(source, &["kind", "values", "first_note"])?;
            result.first_note = integer(source, "first_note", 0)?;
            result.pitches = expressions(source, "values")?
                .into_iter()
                .map(|v| 69.0 + 12.0 * (v - 440.0_f64.log2()) + detune)
                .collect();
        }
        _ => return Err("unsupported tuning source".into()),
    }
    if !result.pitches.iter().all(|v| v.is_finite()) || !result.period_semitones.is_finite() {
        return Err("nonfinite prepared tuning".into());
    }
    Ok(result)
}

pub fn number(table: &Table, name: &str, default: f64) -> Result<f64, String> {
    match table.get(name) {
        None => Ok(default),
        Some(v) => v
            .as_float()
            .or_else(|| v.as_integer().map(|v| v as f64))
            .or_else(|| {
                v.as_str().and_then(|s| {
                    s.trim()
                        .trim_end_matches("musical_cent")
                        .trim_end_matches("cents")
                        .trim()
                        .parse()
                        .ok()
                })
            })
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("invalid {name}")),
    }
}
fn integer(table: &Table, name: &str, default: i64) -> Result<i64, String> {
    table.get(name).map_or(Ok(default), |v| {
        v.as_integer().ok_or_else(|| format!("invalid {name}"))
    })
}
fn text<'a>(table: &'a Table, name: &str, default: &'a str) -> Result<&'a str, String> {
    table.get(name).map_or(Ok(default), |v| {
        v.as_str().ok_or_else(|| format!("invalid {name}"))
    })
}
fn fields(table: &Table, allowed: &[&str]) -> Result<(), String> {
    if table.keys().any(|k| !allowed.contains(&k.as_str())) {
        Err("unsupported scale or tuning field".into())
    } else {
        Ok(())
    }
}
fn expressions(table: &Table, name: &str) -> Result<Vec<f64>, String> {
    let values = table
        .get(name)
        .and_then(Value::as_array)
        .ok_or("tuning values must be a list")?;
    if values.is_empty() {
        return Err("tuning list must not be empty".into());
    }
    values
        .iter()
        .map(|v| pitch_logarithm(v.as_str().ok_or("tuning values must be expressions")?))
        .collect()
}
fn spelling(name: char, offset: i64, sharp: bool, accidentals: &str) -> String {
    if accidentals == "none" {
        return name.to_string();
    }
    let large = if sharp { "♯" } else { "♭" };
    if accidentals == "half" {
        format!(
            "{name}{}{}",
            large.repeat(offset as usize / 2),
            if offset % 2 == 0 {
                ""
            } else if sharp {
                "^"
            } else {
                "v"
            }
        )
    } else {
        format!("{name}{}", large.repeat(offset as usize))
    }
}

pub fn pitch_logarithm(text: &str) -> Result<f64, String> {
    let text = text
        .trim()
        .trim_end_matches("hertz")
        .trim_end_matches("Hz")
        .trim();
    if text.len() > 4096
        || !text
            .chars()
            .all(|c| c.is_ascii_digit() || ".eE/+^-() \t\r\n".contains(c))
    {
        return Err("invalid pitch expression".into());
    }
    let mut parser = Expression {
        bytes: text.as_bytes(),
        at: 0,
        depth: 0,
    };
    let value = parser.division()?;
    parser.space();
    if parser.at != parser.bytes.len() || value.sign != 1 || !value.logarithm.is_finite() {
        return Err("pitch expression must be finite and positive".into());
    }
    Ok(value.logarithm)
}

#[derive(Clone, Copy)]
struct Number {
    value: f64,
    sign: i8,
    logarithm: f64,
}
struct Expression<'a> {
    bytes: &'a [u8],
    at: usize,
    depth: usize,
}
impl Expression<'_> {
    fn space(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }
    fn take(&mut self, c: u8) -> bool {
        self.space();
        if self.bytes.get(self.at) == Some(&c) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn division(&mut self) -> Result<Number, String> {
        let mut value = self.unary()?;
        while self.take(b'/') {
            let other = self.unary()?;
            if other.sign == 0 {
                return Err("division by zero in pitch expression".into());
            }
            value.sign *= other.sign;
            value.logarithm -= other.logarithm;
            value.value /= other.value;
        }
        Ok(value)
    }
    fn unary(&mut self) -> Result<Number, String> {
        self.depth += 1;
        if self.depth > 64 {
            return Err("pitch expression exceeds depth limit".into());
        }
        let value = if self.take(b'-') {
            let mut value = self.unary()?;
            value.sign = -value.sign;
            value.value = -value.value;
            value
        } else if self.take(b'+') {
            self.unary()?
        } else {
            self.power()?
        };
        self.depth -= 1;
        Ok(value)
    }
    fn power(&mut self) -> Result<Number, String> {
        let mut base = self.atom()?;
        if self.take(b'^') {
            let value = self.unary()?;
            let exponent = if value.sign == 0 {
                0.0
            } else if value.value.is_finite() {
                value.value
            } else {
                f64::from(value.sign) * value.logarithm.exp2()
            };
            if !exponent.is_finite() || exponent.abs() > 1024.0 {
                return Err("pitch exponent exceeds 1024".into());
            }
            if base.sign == 0 && exponent < 0.0 || base.sign < 0 && exponent.fract() != 0.0 {
                return Err("undefined pitch power".into());
            }
            if exponent == 0.0 {
                return Ok(Number {
                    sign: 1,
                    value: 1.0,
                    logarithm: 0.0,
                });
            }
            if base.sign < 0 {
                base.sign = if exponent.rem_euclid(2.0) == 0.0 {
                    1
                } else {
                    -1
                };
            }
            base.value = base.value.powf(exponent);
            base.logarithm *= exponent;
        }
        Ok(base)
    }
    fn atom(&mut self) -> Result<Number, String> {
        if self.take(b'(') {
            let value = self.division()?;
            if !self.take(b')') {
                return Err("missing pitch parenthesis".into());
            }
            return Ok(value);
        }
        self.space();
        let start = self.at;
        while self
            .bytes
            .get(self.at)
            .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
        {
            self.at += 1;
        }
        if self
            .bytes
            .get(self.at)
            .is_some_and(|c| *c == b'e' || *c == b'E')
        {
            self.at += 1;
            if self
                .bytes
                .get(self.at)
                .is_some_and(|c| *c == b'+' || *c == b'-')
            {
                self.at += 1;
            }
            while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                self.at += 1;
            }
        }
        let literal =
            std::str::from_utf8(&self.bytes[start..self.at]).map_err(|_| "invalid pitch number")?;
        if let Ok(value) = literal.parse::<f64>() {
            if value.is_finite() && value > 0.0 {
                return Ok(Number {
                    sign: 1,
                    value,
                    logarithm: value.log2(),
                });
            }
        }
        let parts: Vec<_> = literal.split(['e', 'E']).collect();
        if parts.len() > 2 || parts[0].matches('.').count() > 1 {
            return Err("invalid pitch number".into());
        }
        let exponent = if parts.len() == 2 {
            parts[1]
                .parse::<i32>()
                .map_err(|_| "invalid pitch exponent")?
        } else {
            0
        };
        if !(-4096..=4096).contains(&exponent) {
            return Err("pitch numeric exponent exceeds 4096".into());
        }
        let fraction = parts[0].split('.').nth(1).map_or(0, str::len);
        let digits = parts[0].replace('.', "");
        if digits.is_empty() {
            return Err("invalid pitch number".into());
        }
        let digits = digits.trim_start_matches('0');
        if digits.is_empty() {
            return Ok(Number {
                sign: 0,
                value: 0.0,
                logarithm: 0.0,
            });
        }
        let used = digits.len().min(16);
        let leading = digits[..used]
            .parse::<f64>()
            .map_err(|_| "invalid pitch number")?;
        let decimal_power =
            digits.len() as f64 - used as f64 - fraction as f64 + f64::from(exponent);
        Ok(Number {
            value: literal.parse::<f64>().unwrap_or(f64::NAN),
            sign: 1,
            logarithm: leading.log2() + decimal_power * 10.0_f64.log2(),
        })
    }
}
