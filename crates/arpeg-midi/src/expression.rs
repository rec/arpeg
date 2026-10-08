//! Explicit lane sources and normalized MIDI destination encoding.

use std::collections::BTreeMap;

use arpeg_core::{Beat, ports::InputPort};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lane {
    Breath,
    Bend,
    Pressure,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Current,
    Recorded,
    Motion,
}

pub fn control_lane(port: InputPort) -> Option<Lane> {
    match port {
        InputPort::Breath => Some(Lane::Breath),
        InputPort::Bend => Some(Lane::Bend),
        InputPort::Pressure => Some(Lane::Pressure),
        _ => None,
    }
}

pub fn lane(data: &[u8]) -> Option<Lane> {
    match data {
        [0xb0, 2, _] => Some(Lane::Breath),
        [0xe0, _, _] => Some(Lane::Bend),
        [0xd0, _] => Some(Lane::Pressure),
        _ => None,
    }
}

pub fn motion_message(lane: Lane, value: Beat) -> Result<Vec<u8>, &'static str> {
    let lower = if lane == Lane::Bend { -1 } else { 0 };
    if value < lower.into() || value > 1.into() {
        return Err("expression sample is outside its normalized range");
    }
    let (base, scale) = if lane == Lane::Bend {
        (8192, if value < 0.into() { 8192 } else { 8191 })
    } else {
        (0, 127)
    };
    let denominator = i128::from(*value.denom());
    let numerator = base * denominator + i128::from(*value.numer()) * scale;
    let level = ((2 * numerator + denominator) / (2 * denominator)) as u16;
    Ok(match lane {
        Lane::Breath => vec![0xb0, 2, level as u8],
        Lane::Bend => vec![0xe0, (level & 127) as u8, (level >> 7) as u8],
        Lane::Pressure => vec![0xd0, level as u8],
    })
}

pub fn entry_messages(
    sources: &BTreeMap<Lane, Source>,
    current: &BTreeMap<u8, Vec<u8>>,
    motion: &BTreeMap<Lane, Beat>,
) -> Vec<Vec<u8>> {
    let mut output = Vec::new();
    for (lane, status) in [
        (Lane::Breath, 0xb0),
        (Lane::Bend, 0xe0),
        (Lane::Pressure, 0xd0),
    ] {
        match sources[&lane] {
            Source::Current => {
                if let Some(data) = current.get(&status) {
                    output.push(data.clone());
                }
            }
            Source::Motion => {
                if let Some(value) = motion.get(&lane) {
                    output.push(motion_message(lane, *value).expect("validated Motion sample"));
                }
            }
            Source::Recorded => {}
        }
    }
    output
}
