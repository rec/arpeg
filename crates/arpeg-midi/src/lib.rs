//! File and profile adapters for the portable arpeggiator core.

use std::collections::{HashMap, VecDeque};
use std::path::Path;

use arpeg_core::bank::CaptureMode;
use arpeg_core::chance::Chance;
use arpeg_core::live::Retrigger;
use arpeg_core::ports::PitchBoundary;
use arpeg_core::rhythm::{PatternStep, Rhythm};
use arpeg_core::{Bank, Beat, HeldNote, Selection, Walk, render_held};
use midly::{
    Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
    num::{u4, u7, u28},
};

#[cfg(feature = "device-host")]
pub mod live;
pub mod player;

pub struct HeldProfile {
    pub selection_offset: i64,
    pub offset_rest_outside: bool,
    pub transposition: i64,
    pub pitch_boundary: PitchBoundary,
    pub chance: Chance,
    pub bank: Bank,
    pub selection: Selection,
    pub rhythm: Rhythm,
    pub gate: Beat,
    pub retrigger: Retrigger,
}

pub struct CapturedProfile {
    pub selection_offset: i64,
    pub offset_rest_outside: bool,
    pub transposition: i64,
    pub pitch_boundary: PitchBoundary,
    pub chance: Chance,
    pub mode: CaptureMode,
    pub selection: Selection,
    pub step: Beat,
    pub gate: Beat,
    pub retrigger: Retrigger,
    pub current_expression: bool,
}

pub enum Profile {
    Classic(HeldProfile),
    Captured(CapturedProfile),
}

pub fn parse_profile(text: &str, path: Option<&Path>) -> Result<Profile, String> {
    let score: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let score = score.as_table().ok_or("profile must be a TOML table")?;
    if score.keys().any(|key| {
        !["format", "version", "name", "title", "tags", "kind", "body"].contains(&key.as_str())
    }) || score
        .get("format")
        .is_some_and(|value| value.as_str() != Some("recs"))
        || score
            .get("version")
            .is_some_and(|value| value.as_integer() != Some(4))
    {
        return Err("unsupported arpeggiator document header".into());
    }
    if score
        .get("kind")
        .is_some_and(|v| v.as_str() != Some("arpeggiator"))
    {
        return Err("profile kind must be arpeggiator".into());
    }
    let name = match score.get("name") {
        Some(value) => value.as_str().ok_or("profile name must be a string")?,
        None => path
            .ok_or("profile requires name when no source file is supplied")?
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("profile requires a UTF-8 filename stem")?,
    };
    if name.is_empty()
        || name != name.trim()
        || name.contains([':', '#', '/'])
        || name.contains(".*")
    {
        return Err("invalid profile name".into());
    }
    if score
        .get("title")
        .and_then(toml::Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err("profile requires a nonempty title".into());
    }
    if score.get("tags").is_some_and(|value| {
        value.as_array().is_none_or(|tags| {
            tags.iter().any(|tag| {
                tag.as_str().is_none_or(|tag| {
                    !tag.starts_with('#')
                        || tag.len() == 1
                        || tag[1..]
                            .chars()
                            .any(|c| c.is_whitespace() || ":#/".contains(c))
                })
            })
        })
    }) {
        return Err("invalid profile tags".into());
    }
    let body = score
        .get("body")
        .and_then(toml::Value::as_table)
        .ok_or("profile requires a body")?;
    if body.keys().any(|key| {
        ![
            "bank",
            "selection",
            "rhythm",
            "gate",
            "retrigger",
            "expression",
            "seed",
            "probability",
            "transposition",
            "selection_offset",
        ]
        .contains(&key.as_str())
    }) {
        return Err("profile contains an unsupported body field".into());
    }
    enum ParsedBank {
        Classic(Bank),
        Captured(CaptureMode),
    }
    let bank = match body.get("bank") {
        None => ParsedBank::Classic(Bank::Held),
        Some(value) => {
            let bank = value.as_table().ok_or("bank must be a table")?;
            match bank.get("kind").and_then(toml::Value::as_str) {
                Some("held") if bank.len() == 1 => ParsedBank::Classic(Bank::Held),
                Some("latched")
                    if bank
                        .keys()
                        .all(|key| ["kind", "update"].contains(&key.as_str())) =>
                {
                    match bank.get("update") {
                        None => ParsedBank::Classic(Bank::LatchedReplace),
                        Some(value) if value.as_str() == Some("replace") => {
                            ParsedBank::Classic(Bank::LatchedReplace)
                        }
                        Some(value) if value.as_str() == Some("add") => {
                            ParsedBank::Classic(Bank::LatchedAdd)
                        }
                        Some(value) if value.as_str() == Some("toggle") => {
                            ParsedBank::Classic(Bank::LatchedToggle)
                        }
                        _ => return Err("unsupported latch update".into()),
                    }
                }
                Some("history")
                    if bank
                        .keys()
                        .all(|key| ["kind", "notes", "publish"].contains(&key.as_str())) =>
                {
                    if bank
                        .get("publish")
                        .is_some_and(|value| value.as_str() != Some("step"))
                    {
                        return Err("history bank must publish at steps".into());
                    }
                    let notes = bank
                        .get("notes")
                        .map_or(Some(8), toml::Value::as_integer)
                        .ok_or("history notes must be a positive integer")?;
                    if notes <= 0 {
                        return Err("history notes must be a positive integer".into());
                    }
                    ParsedBank::Captured(CaptureMode::History(
                        usize::try_from(notes).map_err(|_| "history notes are too large")?,
                    ))
                }
                Some("phrase")
                    if bank
                        .keys()
                        .all(|k| ["kind", "publish"].contains(&k.as_str())) =>
                {
                    if bank
                        .get("publish")
                        .is_some_and(|v| v.as_str() != Some("step"))
                    {
                        return Err("phrase bank must publish at steps".into());
                    }
                    ParsedBank::Captured(CaptureMode::Phrase)
                }
                _ => return Err("unsupported note bank".into()),
            }
        }
    };
    let retrigger = match body.get("retrigger") {
        None => Retrigger::OnEmpty,
        Some(value) if value.as_str() == Some("on_empty") => Retrigger::OnEmpty,
        Some(value) if value.as_str() == Some("bank_edit") => Retrigger::BankEdit,
        _ => return Err("unsupported retrigger policy".into()),
    };
    let expression = match body.get("expression") {
        Some(value) => Some(value.as_table().ok_or("expression must be a table")?),
        None => None,
    };
    match &bank {
        ParsedBank::Classic(_) => {
            if let Some(expression) = expression {
                if expression
                    .keys()
                    .any(|key| !["source", "timing", "gaps"].contains(&key.as_str()))
                    || expression
                        .get("source")
                        .is_some_and(|value| value.as_str() != Some("current"))
                    || expression
                        .get("timing")
                        .is_some_and(|value| value.as_str() != Some("original"))
                    || expression
                        .get("gaps")
                        .is_some_and(|value| value.as_str() != Some("omit"))
                {
                    return Err("unsupported expression policy".into());
                }
            }
        }
        ParsedBank::Captured(_) => {
            let expression =
                expression.ok_or("captured playback requires recorded, fit, carry expression")?;
            if expression
                .keys()
                .any(|key| !["source", "timing", "gaps"].contains(&key.as_str()))
                || !matches!(
                    expression.get("source").and_then(toml::Value::as_str),
                    Some("recorded" | "current")
                )
                || expression.get("timing").and_then(toml::Value::as_str) != Some("fit")
                || expression.get("gaps").and_then(toml::Value::as_str) != Some("carry")
            {
                return Err(
                    "captured playback requires recorded or current, fit, carry expression".into(),
                );
            }
        }
    }
    let selection = match body.get("selection") {
        Some(value) => Some(value.as_table().ok_or("selection must be a table")?),
        None => None,
    };
    let selection = match selection
        .and_then(|selection| selection.get("kind"))
        .and_then(toml::Value::as_str)
    {
        None if selection.is_none() => Selection::Ascending,
        Some("ascending" | "descending") => {
            let selection = selection.expect("selection table");
            if selection
                .get("key")
                .is_some_and(|value| value.as_str() != Some("pitch"))
                || selection
                    .get("repeats")
                    .is_some_and(|value| value.as_integer() != Some(1))
                || selection
                    .keys()
                    .any(|key| !["kind", "key", "repeats"].contains(&key.as_str()))
            {
                return Err("unsupported pitch selection option".into());
            }
            if selection.get("kind").and_then(toml::Value::as_str) == Some("ascending") {
                Selection::Ascending
            } else {
                if selection.contains_key("repeats") {
                    return Err("descending selection has no repeats option".into());
                }
                Selection::Descending
            }
        }
        Some("played") => {
            let selection = selection.expect("selection table");
            if selection
                .keys()
                .any(|key| !["kind", "direction"].contains(&key.as_str()))
            {
                return Err("unsupported played selection option".into());
            }
            match selection.get("direction") {
                None => Selection::Played,
                Some(value) if value.as_str() == Some("forward") => Selection::Played,
                Some(value) if value.as_str() == Some("reverse") => Selection::ReversePlayed,
                _ => return Err("unsupported played direction".into()),
            }
        }
        Some(kind @ ("inside_out" | "outside_in")) => {
            if selection.expect("selection table").len() != 1 {
                return Err("unsupported center/edge selection option".into());
            }
            if kind == "inside_out" {
                Selection::InsideOut
            } else {
                Selection::OutsideIn
            }
        }
        Some("alternating") => {
            let table = selection.expect("selection table");
            if table
                .keys()
                .any(|key| !["kind", "repeat_endpoints"].contains(&key.as_str()))
            {
                return Err("unsupported alternating option".into());
            }
            let repeat_endpoints = table
                .get("repeat_endpoints")
                .map(|value| value.as_bool().ok_or("repeat_endpoints must be a boolean"))
                .transpose()?
                .unwrap_or(false);
            Selection::Alternating { repeat_endpoints }
        }
        Some("index_pattern") => {
            let table = selection.expect("selection table");
            if table
                .keys()
                .any(|key| !["kind", "indices", "boundary"].contains(&key.as_str()))
            {
                return Err("unsupported index pattern option".into());
            }
            let indices = table
                .get("indices")
                .and_then(toml::Value::as_array)
                .ok_or("index pattern requires an indices array")?
                .iter()
                .map(|value| {
                    value
                        .as_integer()
                        .and_then(|index| u64::try_from(index).ok())
                        .ok_or("indices must be nonnegative integers")
                })
                .collect::<Result<Vec<_>, _>>()?;
            if indices.is_empty() {
                return Err("index pattern requires at least one index".into());
            }
            let rest_outside = match table.get("boundary") {
                None => false,
                Some(value) if value.as_str() == Some("wrap") => false,
                Some(value) if value.as_str() == Some("rest") => true,
                _ => return Err("index pattern boundary must be wrap or rest".into()),
            };
            Selection::IndexPattern {
                indices,
                rest_outside,
            }
        }
        Some("choice") => {
            let table = selection.expect("selection table");
            if table
                .keys()
                .any(|key| !["kind", "weights", "extend", "no_repeat"].contains(&key.as_str()))
            {
                return Err("unsupported choice option".into());
            }
            let weights = match table.get("weights") {
                None => vec![1],
                Some(value) => value
                    .as_array()
                    .ok_or("choice weights must be an array")?
                    .iter()
                    .map(|value| {
                        value
                            .as_integer()
                            .and_then(|weight| u32::try_from(weight).ok())
                            .filter(|weight| *weight > 0)
                            .ok_or("choice weights must be positive u32 integers")
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            };
            if weights.is_empty() {
                return Err("choice requires positive weights".into());
            }
            let repeat_weights = match table.get("extend") {
                None => false,
                Some(value) if value.as_str() == Some("ones") => false,
                Some(value) if value.as_str() == Some("repeat") => true,
                _ => return Err("choice extend must be ones or repeat".into()),
            };
            let no_repeat = table
                .get("no_repeat")
                .map(|value| value.as_bool().ok_or("no_repeat must be a boolean"))
                .transpose()?
                .unwrap_or(false);
            Selection::Choice {
                weights,
                repeat_weights,
                no_repeat,
            }
        }
        Some("shuffle") => {
            let table = selection.expect("selection table");
            if table
                .keys()
                .any(|key| !["kind", "mode", "no_repeat", "on_edit"].contains(&key.as_str()))
            {
                return Err("unsupported shuffle option".into());
            }
            let once = match table.get("mode") {
                None => false,
                Some(value) if value.as_str() == Some("cycle") => false,
                Some(value) if value.as_str() == Some("once") => true,
                _ => return Err("shuffle mode must be once or cycle".into()),
            };
            let preserve = match table.get("on_edit") {
                None => false,
                Some(value) if value.as_str() == Some("restart") => false,
                Some(value) if value.as_str() == Some("preserve") => true,
                _ => return Err("shuffle on_edit must be restart or preserve".into()),
            };
            let no_repeat = table
                .get("no_repeat")
                .map(|value| value.as_bool().ok_or("no_repeat must be a boolean"))
                .transpose()?
                .unwrap_or(false);
            Selection::Shuffle {
                once,
                no_repeat,
                preserve,
            }
        }
        Some("walk") => {
            let table = selection.expect("selection table");
            if table.keys().any(|key| {
                !["kind", "moves", "weights", "boundary", "start", "on_remove"]
                    .contains(&key.as_str())
            }) || table
                .get("boundary")
                .is_some_and(|value| value.as_str() != Some("wrap"))
            {
                return Err("unsupported walk option".into());
            }
            let moves = table
                .get("moves")
                .and_then(toml::Value::as_array)
                .ok_or("walk requires moves")?
                .iter()
                .map(|v| v.as_integer().ok_or("walk moves must be integers"))
                .collect::<Result<Vec<_>, _>>()?;
            let weights = table
                .get("weights")
                .and_then(toml::Value::as_array)
                .ok_or("walk requires weights")?
                .iter()
                .map(|v| {
                    v.as_integer()
                        .and_then(|w| u64::try_from(w).ok())
                        .ok_or("walk weights must be positive integers")
                })
                .collect::<Result<Vec<_>, _>>()?;
            let start_move = match table.get("start").and_then(toml::Value::as_str) {
                None if !table.contains_key("start") => false,
                Some("lowest") => false,
                Some("move") => true,
                _ => return Err("unsupported walk start".into()),
            };
            let keep_rank = match table.get("on_remove").and_then(toml::Value::as_str) {
                None if !table.contains_key("on_remove") => false,
                Some("lowest") => false,
                Some("rank") => true,
                _ => return Err("unsupported walk removal policy".into()),
            };
            let walk = Walk {
                moves,
                weights,
                start_move,
                keep_rank,
            };
            walk.validate().map_err(str::to_owned)?;
            Selection::Walk(walk)
        }
        _ => return Err("unsupported note selection".into()),
    };
    let rhythm = parse_rhythm(body)?;
    let gate = parse_gate(body)?;
    let probability = match body.get("probability") {
        None => Beat::from_integer(1),
        Some(toml::Value::String(value)) => parse_ratio(value)?,
        Some(toml::Value::Integer(value)) => Beat::from_integer(*value),
        _ => return Err("probability must be an exact rational".into()),
    };
    let seed = body
        .get("seed")
        .map(|value| value.as_integer().ok_or("seed must be an integer"))
        .transpose()?;
    let chance = Chance {
        probability,
        seed,
        name: name.to_owned(),
    };
    chance.validate().map_err(str::to_owned)?;
    if matches!(&selection, Selection::Walk(walk) if walk.moves.len() > 1) && seed.is_none() {
        return Err("weighted walk requires an explicit seed".into());
    }
    if matches!(selection, Selection::Shuffle { .. }) && seed.is_none() {
        return Err("shuffle requires an explicit seed".into());
    }
    if matches!(selection, Selection::Choice { .. }) && seed.is_none() {
        return Err("choice requires an explicit seed".into());
    }
    let (transposition, pitch_boundary) = match body.get("transposition") {
        None => (0, PitchBoundary::Drop),
        Some(value) => {
            let table = value.as_table().ok_or("transposition must be a table")?;
            if table
                .keys()
                .any(|k| !["semitones", "boundary"].contains(&k.as_str()))
            {
                return Err("unsupported transposition field".into());
            }
            let semitones = match table.get("semitones") {
                None => 0,
                Some(value) => value
                    .as_integer()
                    .ok_or("transposition requires whole semitones")?,
            };
            let boundary = match table.get("boundary") {
                None => PitchBoundary::Drop,
                Some(value) => match value.as_str() {
                    Some("drop") => PitchBoundary::Drop,
                    Some("fold") => PitchBoundary::Fold,
                    Some("error") => PitchBoundary::Error,
                    _ => return Err("transposition boundary must be drop, fold, or error".into()),
                },
            };
            (semitones, boundary)
        }
    };
    let (selection_offset, offset_rest_outside) = match body.get("selection_offset") {
        None => (0, false),
        Some(value) => {
            let table = value.as_table().ok_or("selection offset must be a table")?;
            if table
                .keys()
                .any(|k| !["ranks", "boundary"].contains(&k.as_str()))
            {
                return Err("unsupported selection offset field".into());
            }
            let ranks = match table.get("ranks") {
                None => 0,
                Some(value) => value
                    .as_integer()
                    .ok_or("selection offset requires whole ranks")?,
            };
            let rest = match table.get("boundary") {
                None => false,
                Some(value) => match value.as_str() {
                    Some("wrap") => false,
                    Some("rest") => true,
                    _ => return Err("selection offset boundary must be wrap or rest".into()),
                },
            };
            (ranks, rest)
        }
    };
    Ok(match bank {
        ParsedBank::Classic(bank) => Profile::Classic(HeldProfile {
            selection_offset,
            offset_rest_outside,
            transposition,
            pitch_boundary,
            chance,
            bank,
            selection,
            rhythm,
            gate,
            retrigger,
        }),
        ParsedBank::Captured(mode) => {
            if matches!(selection, Selection::Choice { .. }) {
                return Err("choice selection currently requires a held or latched bank".into());
            }
            if matches!(selection, Selection::Shuffle { .. }) {
                return Err("shuffle selection currently requires a held or latched bank".into());
            }
            if matches!(selection, Selection::IndexPattern { .. }) {
                return Err(
                    "index pattern selection currently requires a held or latched bank".into(),
                );
            }
            if matches!(selection, Selection::InsideOut | Selection::OutsideIn) {
                return Err(
                    "center/edge selection currently requires a held or latched bank".into(),
                );
            }
            if matches!(selection, Selection::Alternating { .. }) {
                return Err("alternating currently requires a held or latched bank".into());
            }
            if matches!(selection, Selection::Walk(_)) {
                return Err("walk currently requires a held or latched bank".into());
            }
            if chance.probability != Beat::from_integer(1) {
                return Err("probability currently requires a held or latched bank".into());
            }
            let Rhythm::Grid { step } = rhythm else {
                return Err("captured playback currently requires grid rhythm".into());
            };
            Profile::Captured(CapturedProfile {
                selection_offset,
                offset_rest_outside,
                transposition,
                pitch_boundary,
                chance,
                mode,
                selection,
                step,
                gate,
                retrigger,
                current_expression: expression
                    .and_then(|e| e.get("source"))
                    .and_then(toml::Value::as_str)
                    == Some("current"),
            })
        }
    })
}

fn parse_rhythm(body: &toml::map::Map<String, toml::Value>) -> Result<Rhythm, String> {
    let rhythm = body
        .get("rhythm")
        .and_then(toml::Value::as_table)
        .ok_or("profile requires rhythm")?;
    let parsed = match rhythm.get("kind").and_then(toml::Value::as_str) {
        Some("grid") if rhythm.len() == 2 => Rhythm::Grid {
            step: parse_beat_duration(rhythm.get("step"))?,
        },
        Some("euclidean")
            if rhythm.keys().all(|key| {
                ["kind", "step", "steps", "pulses", "rotation"].contains(&key.as_str())
            }) =>
        {
            let steps = rhythm
                .get("steps")
                .and_then(toml::Value::as_integer)
                .ok_or("Euclidean steps must be an integer")?;
            let pulses = rhythm
                .get("pulses")
                .and_then(toml::Value::as_integer)
                .ok_or("Euclidean pulses must be an integer")?;
            let rotation = rhythm
                .get("rotation")
                .map_or(Some(0), toml::Value::as_integer)
                .ok_or("Euclidean rotation must be an integer")?;
            Rhythm::Euclidean {
                step: parse_beat_duration(rhythm.get("step"))?,
                steps,
                pulses,
                rotation,
            }
        }
        Some("pattern") if rhythm.len() == 2 => {
            let steps = rhythm
                .get("steps")
                .and_then(toml::Value::as_array)
                .ok_or("pattern requires a steps array")?;
            let mut parsed = Vec::new();
            for value in steps {
                let entry = value.as_table().ok_or("pattern step must be a table")?;
                let duration = parse_beat_duration(entry.get("duration"))?;
                let step = match entry.get("kind").and_then(toml::Value::as_str) {
                    Some("hit")
                        if entry
                            .keys()
                            .all(|key| ["kind", "duration", "repeats"].contains(&key.as_str())) =>
                    {
                        let repeats = entry
                            .get("repeats")
                            .map_or(Some(1), toml::Value::as_integer)
                            .ok_or("repeats must be a positive integer")?;
                        if repeats <= 0 {
                            return Err("repeats must be a positive integer".into());
                        }
                        PatternStep::Hit {
                            duration,
                            repeats: usize::try_from(repeats)
                                .map_err(|_| "repeat count is too large")?,
                        }
                    }
                    Some("rest") if entry.len() == 2 => PatternStep::Rest { duration },
                    Some("tie") if entry.len() == 2 => PatternStep::Tie { duration },
                    _ => return Err("unsupported pattern step".into()),
                };
                parsed.push(step);
            }
            Rhythm::Pattern { steps: parsed }
        }
        _ => return Err("only grid, Euclidean, or pattern rhythm is supported".into()),
    };
    parsed.validate().map_err(str::to_owned)?;
    Ok(parsed)
}

fn parse_beat_duration(value: Option<&toml::Value>) -> Result<Beat, String> {
    let text = value
        .and_then(toml::Value::as_str)
        .and_then(|value| value.strip_suffix(" beat"))
        .ok_or("duration must be a rational beat duration")?;
    parse_ratio(text)
}

fn parse_gate(body: &toml::map::Map<String, toml::Value>) -> Result<Beat, String> {
    let gate = match body.get("gate") {
        Some(toml::Value::String(value)) => parse_ratio(value)?,
        Some(toml::Value::Integer(value)) => Beat::from_integer(*value),
        None => Beat::new(4, 5),
        _ => return Err("gate must be an exact rational".into()),
    };
    if gate < Beat::from_integer(0) {
        return Err("gate must be nonnegative".into());
    }
    Ok(gate)
}

pub fn render_file(profile: &str, input: &[u8], path: Option<&Path>) -> Result<Vec<u8>, String> {
    let profile = match parse_profile(profile, path)? {
        Profile::Classic(p) => p,
        Profile::Captured(p) => {
            return Err(if p.mode == CaptureMode::Phrase {
                "phrase profiles require live MIDI input"
            } else {
                "history profiles require live MIDI input"
            }
            .into());
        }
    };
    if profile.chance.probability != Beat::from_integer(1) {
        return Err("probability currently requires live input".into());
    }
    if profile.transposition != 0 {
        return Err("transposition currently requires live input".into());
    }
    if profile.selection_offset != 0 {
        return Err("selection offset currently requires live input".into());
    }
    if profile.retrigger != Retrigger::OnEmpty {
        return Err("file rendering does not support bank-edit retrigger".into());
    }
    let file = Smf::parse(input).map_err(|e| e.to_string())?;
    if file.header.format != Format::SingleTrack || file.tracks.len() != 1 {
        return Err("only single-track MIDI files are supported".into());
    }
    let Timing::Metrical(ticks_per_beat) = file.header.timing else {
        return Err("only metrical MIDI timing is supported".into());
    };
    let ticks_per_beat = ticks_per_beat.as_int();
    let mut notes = Vec::new();
    let mut active: HashMap<(u8, u8), VecDeque<usize>> = HashMap::new();
    let mut tempo = Vec::new();
    let mut at = 0u32;
    let mut end = None;
    for event in &file.tracks[0] {
        if end.is_some() {
            return Err("MIDI events occur after end-of-track".into());
        }
        at = at
            .checked_add(event.delta.as_int())
            .ok_or("MIDI time overflow")?;
        match event.kind {
            TrackEventKind::Midi {
                channel,
                message: MidiMessage::NoteOn { key, vel },
            } if vel.as_int() != 0 => {
                let index = notes.len();
                notes.push(InputNote {
                    id: format!("n{index}"),
                    key,
                    channel,
                    velocity: vel,
                    onset: at,
                    release: None,
                    release_message: None,
                });
                active
                    .entry((channel.as_int(), key.as_int()))
                    .or_default()
                    .push_back(index);
            }
            TrackEventKind::Midi {
                channel,
                message:
                    message @ (MidiMessage::NoteOff { key, .. } | MidiMessage::NoteOn { key, vel: _ }),
            } => {
                let index = active
                    .get_mut(&(channel.as_int(), key.as_int()))
                    .and_then(VecDeque::pop_front)
                    .ok_or("MIDI release has no matching onset")?;
                notes[index].release = Some(at);
                notes[index].release_message = Some(message);
            }
            TrackEventKind::Meta(MetaMessage::Tempo(value)) => tempo.push((at, value)),
            TrackEventKind::Meta(MetaMessage::EndOfTrack) => end = Some(at),
            _ => return Err("unsupported MIDI event in held-note input".into()),
        }
    }
    let end = end.ok_or("MIDI file has no end-of-track event")?;
    if notes.iter().any(|note| note.release.is_none()) {
        return Err("MIDI input has unreleased notes".into());
    }
    let held: Vec<_> = notes
        .iter()
        .map(|note| HeldNote {
            id: &note.id,
            key: i32::from(note.key.as_int()),
            onset: Beat::new(i64::from(note.onset), i64::from(ticks_per_beat)),
            release: Beat::new(
                i64::from(note.release.expect("checked release")),
                i64::from(ticks_per_beat),
            ),
        })
        .collect();
    let through = Beat::new(i64::from(end), i64::from(ticks_per_beat));
    let occurrences = render_held(
        &held,
        profile.bank,
        profile.selection,
        profile.rhythm,
        profile.gate,
        through,
    )
    .map_err(str::to_owned)?;
    let mut events: Vec<TimedEvent> = tempo
        .into_iter()
        .map(|(tick, value)| TimedEvent {
            tick,
            priority: 0,
            decision: 0,
            phase: 0,
            kind: TrackEventKind::Meta(MetaMessage::Tempo(value)),
        })
        .collect();
    let mut owned_end = HashMap::new();
    for occurrence in &occurrences {
        let note = notes
            .iter()
            .find(|note| note.id == occurrence.source_id)
            .expect("source note");
        let key = (note.channel.as_int(), note.key.as_int());
        if owned_end
            .get(&key)
            .is_some_and(|end| *end > occurrence.onset)
        {
            return Err("overlapping same-key output requires an allocation policy".into());
        }
        owned_end.insert(key, occurrence.gate_end);
        let onset = round_tick(occurrence.onset, ticks_per_beat)?;
        let gate_end = round_tick(occurrence.gate_end, ticks_per_beat)?;
        events.push(TimedEvent {
            tick: onset,
            priority: 1,
            decision: occurrence.decision,
            phase: 0,
            kind: TrackEventKind::Midi {
                channel: note.channel,
                message: MidiMessage::NoteOn {
                    key: note.key,
                    vel: note.velocity,
                },
            },
        });
        events.push(TimedEvent {
            tick: gate_end,
            priority: if gate_end == onset { 1 } else { 0 },
            decision: occurrence.decision,
            phase: if gate_end == onset { 1 } else { 0 },
            kind: TrackEventKind::Midi {
                channel: note.channel,
                message: note.release_message.expect("checked release"),
            },
        });
    }
    events.sort_by_key(|event| (event.tick, event.priority, event.decision, event.phase));
    let final_tick = events.last().map_or(end, |event| end.max(event.tick));
    events.push(TimedEvent {
        tick: final_tick,
        priority: 3,
        decision: occurrences.len(),
        phase: 0,
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });
    let mut previous = 0;
    let track: Vec<_> = events
        .into_iter()
        .map(|event| {
            let delta = event.tick - previous;
            previous = event.tick;
            Ok(TrackEvent {
                delta: u28::try_from(delta).ok_or("MIDI delta exceeds 28 bits")?,
                kind: event.kind,
            })
        })
        .collect::<Result<_, String>>()?;
    let output = Smf {
        header: Header::new(Format::SingleTrack, Timing::Metrical(ticks_per_beat.into())),
        tracks: vec![track],
    };
    let mut bytes = Vec::new();
    output.write_std(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

struct InputNote {
    id: String,
    key: u7,
    channel: u4,
    velocity: u7,
    onset: u32,
    release: Option<u32>,
    release_message: Option<MidiMessage>,
}

struct TimedEvent<'a> {
    tick: u32,
    priority: u8,
    decision: usize,
    phase: u8,
    kind: TrackEventKind<'a>,
}

fn parse_ratio(text: &str) -> Result<Beat, String> {
    let (numerator, denominator) = text.split_once('/').unwrap_or((text, "1"));
    let numerator = numerator
        .parse::<i64>()
        .map_err(|_| "invalid rational numerator")?;
    let denominator = denominator
        .parse::<i64>()
        .map_err(|_| "invalid rational denominator")?;
    if denominator == 0 {
        return Err("rational denominator must not be zero".into());
    }
    Ok(Beat::new(numerator, denominator))
}

fn round_tick(beat: Beat, ticks_per_beat: u16) -> Result<u32, String> {
    let scaled = beat * i64::from(ticks_per_beat);
    let numerator = *scaled.numer();
    let denominator = *scaled.denom();
    let whole = numerator / denominator;
    let remainder = numerator % denominator;
    let rounded = whole
        + i64::from(
            remainder * 2 > denominator || (remainder * 2 == denominator && whole % 2 != 0),
        );
    u32::try_from(rounded).map_err(|_| "output MIDI tick is out of range".into())
}
