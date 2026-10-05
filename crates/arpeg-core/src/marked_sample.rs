//! Identified source-frame regions with selection keys independent of pitch.

use crate::Selection;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Marker {
    pub note_id: String,
    pub selection_key: i32,
    pub at_frame: i64,
    pub gate_end_frame: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MarkedSample {
    pub capture_id: String,
    pub asset: String,
    pub sample_rate: u32,
    pub frames: i64,
    pub channels: Vec<usize>,
    pub markers: Vec<Marker>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegionNote {
    pub capture_id: String,
    pub note_id: String,
    pub selection_key: i32,
    pub asset: String,
    pub start_frame: i64,
    pub gate_end_frame: i64,
    pub end_frame: i64,
}

impl MarkedSample {
    pub fn notes(&self) -> Result<Vec<RegionNote>, &'static str> {
        if self.sample_rate == 0 || self.frames <= 0 || self.channels.is_empty() {
            return Err("sample rate, frames, and channels must be positive");
        }
        if self
            .channels
            .iter()
            .enumerate()
            .any(|(index, channel)| self.channels[index + 1..].contains(channel))
        {
            return Err("sample channels must be unique");
        }
        if self.markers.first().map(|marker| marker.at_frame) != Some(0) {
            return Err("an exhaustive sample bank requires a marker at frame zero");
        }
        if self
            .markers
            .last()
            .is_some_and(|marker| marker.at_frame >= self.frames)
            || self
                .markers
                .windows(2)
                .any(|pair| pair[1].at_frame <= pair[0].at_frame)
        {
            return Err("sample markers must increase within the asset");
        }
        if self.markers.iter().enumerate().any(|(index, marker)| {
            self.markers[index + 1..]
                .iter()
                .any(|other| other.note_id == marker.note_id)
        }) {
            return Err("sample marker note IDs must be unique");
        }
        if self.markers.iter().enumerate().any(|(index, marker)| {
            let end = self
                .markers
                .get(index + 1)
                .map_or(self.frames, |next| next.at_frame);
            marker
                .gate_end_frame
                .is_some_and(|gate| gate <= marker.at_frame || gate > end)
        }) {
            return Err("sample gate must end within its region");
        }
        Ok(self
            .markers
            .iter()
            .enumerate()
            .map(|(index, marker)| RegionNote {
                capture_id: self.capture_id.clone(),
                note_id: marker.note_id.clone(),
                selection_key: marker.selection_key,
                asset: self.asset.clone(),
                start_frame: marker.at_frame,
                gate_end_frame: marker.gate_end_frame.unwrap_or_else(|| {
                    self.markers
                        .get(index + 1)
                        .map_or(self.frames, |next| next.at_frame)
                }),
                end_frame: self
                    .markers
                    .get(index + 1)
                    .map_or(self.frames, |next| next.at_frame),
            })
            .collect())
    }

    pub fn select(
        &self,
        selection: Selection,
        cycles: usize,
    ) -> Result<Vec<RegionNote>, &'static str> {
        if cycles == 0 {
            return Err("selection requires at least one cycle");
        }
        let mut notes = self.notes()?;
        match selection {
            Selection::Ascending => {
                notes.sort_by_key(|note| (note.selection_key, note.start_frame))
            }
            Selection::Descending => {
                notes.sort_by_key(|note| (-i64::from(note.selection_key), -note.start_frame))
            }
            Selection::Played => {}
            Selection::ReversePlayed => notes.reverse(),
            Selection::Walk(_) => return Err("walk selection currently requires live MIDI input"),
            Selection::Alternating { .. } => {
                return Err("alternating selection currently requires live MIDI input");
            }
            Selection::IndexPattern { .. } => {
                return Err("index pattern selection currently requires live MIDI input");
            }
            Selection::InsideOut | Selection::OutsideIn => {
                return Err("center/edge selection currently requires live MIDI input");
            }
        }
        Ok((0..cycles).flat_map(|_| notes.iter().cloned()).collect())
    }
}
