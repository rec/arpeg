//! Parse the shared uFor scale and tuning definitions during preparation.

use arpeg_core::{
    ports::{ScaleMapping, ScaleTransform},
    tuning::PreparedTuning,
};
use toml::{Table, Value};
use ufor::{
    expression::evaluate,
    tuning::{Source, Tuning},
};

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
    let tuning: Tuning = value
        .clone()
        .try_into()
        .map_err(|e| format!("invalid tuning: {e}"))?;
    let detune = tuning.detune_cents() / 100.0;
    let root =
        69.0 + 12.0 * (pitch_logarithm(tuning.root_frequency())? - 440.0_f64.log2()) + detune;
    let mut result = PreparedTuning {
        pitches: Vec::new(),
        first_note: tuning.root_note(),
        period: None,
        period_semitones: 0.0,
        computed_limit: None,
    };
    match tuning.source() {
        Source::Computed(source) => {
            result.period = Some(source.notes_per_octave());
            result.period_semitones = 12.0 * pitch_logarithm(source.octave_ratio())?;
            result.pitches = vec![root];
            result.computed_limit = Some(source.denominator_limit() as u64);
        }
        Source::Ratios(source) => {
            result.pitches = source
                .ratios()
                .iter()
                .map(|v| Ok(root + 12.0 * pitch_logarithm(v)?))
                .collect::<Result<_, String>>()?;
            if let Some(v) = source.repeat_ratio() {
                result.period = Some(result.pitches.len() as i64);
                result.period_semitones = 12.0 * pitch_logarithm(v)?;
            }
        }
        Source::Intervals(source) => {
            result.pitches = vec![root];
            for v in source.intervals() {
                result
                    .pitches
                    .push(result.pitches.last().unwrap() + 12.0 * pitch_logarithm(v)?);
            }
            if source.repeats() {
                result.period = Some(result.pitches.len() as i64 - 1);
                result.period_semitones = result.pitches.pop().unwrap() - root;
            }
        }
        Source::Frequencies(source) => {
            result.first_note = source.first_note();
            result.pitches = source
                .frequencies()
                .iter()
                .map(|v| Ok(69.0 + 12.0 * (pitch_logarithm(v)? - 440.0_f64.log2()) + detune))
                .collect::<Result<_, String>>()?;
        }
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

fn pitch_logarithm(text: &str) -> Result<f64, String> {
    evaluate(text)
        .and_then(ufor::number::Number::positive)
        .map(|v| v.as_f64().log2())
        .map_err(|e| e.to_string())
}
