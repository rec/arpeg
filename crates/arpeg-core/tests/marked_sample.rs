use arpeg_core::{
    Selection,
    marked_sample::{MarkedSample, Marker},
};
use serde_json::Value;

#[test]
fn marked_regions_match_the_python_source_frame_trace() {
    let case: Value =
        serde_json::from_str(include_str!("../../../conformance/marked-sample.json")).unwrap();
    let sample = MarkedSample {
        capture_id: case["capture_id"].as_str().unwrap().into(),
        asset: case["asset"].as_str().unwrap().into(),
        sample_rate: case["sample_rate"].as_u64().unwrap() as u32,
        frames: case["frames"].as_i64().unwrap(),
        channels: case["channels"]
            .as_array()
            .unwrap()
            .iter()
            .map(|channel| channel.as_u64().unwrap() as usize)
            .collect(),
        markers: case["markers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|marker| Marker {
                note_id: marker["note_id"].as_str().unwrap().into(),
                selection_key: marker["selection_key"].as_i64().unwrap() as i32,
                at_frame: marker["at_frame"].as_i64().unwrap(),
                gate_end_frame: marker["gate_end_frame"].as_i64(),
            })
            .collect(),
    };
    for (selection, expected) in [
        (Selection::Ascending, "ascending"),
        (Selection::Descending, "descending"),
    ] {
        let actual: Vec<_> = sample
            .select(selection, 1)
            .unwrap()
            .iter()
            .map(|note| serde_json::json!([note.note_id, note.start_frame, note.end_frame]))
            .collect();
        assert_eq!(actual, case[expected].as_array().unwrap().clone());
    }
    assert_eq!(sample.select(Selection::ReversePlayed, 2).unwrap().len(), 6);
    let gates: Vec<_> = sample
        .notes()
        .unwrap()
        .iter()
        .map(|note| note.gate_end_frame)
        .collect();
    assert_eq!(gates, [12_000, 27_000, 48_000]);
}

#[test]
fn an_exhaustive_region_bank_requires_a_frame_zero_marker() {
    let sample = MarkedSample {
        capture_id: "spoken".into(),
        asset: "voice".into(),
        sample_rate: 48_000,
        frames: 48_000,
        channels: vec![0],
        markers: vec![Marker {
            note_id: "late".into(),
            selection_key: 60,
            at_frame: 1,
            gate_end_frame: None,
        }],
    };
    assert_eq!(
        sample.notes(),
        Err("an exhaustive sample bank requires a marker at frame zero")
    );
}

#[test]
fn a_gate_cannot_extend_into_the_next_region() {
    let sample = MarkedSample {
        capture_id: "spoken".into(),
        asset: "voice".into(),
        sample_rate: 48_000,
        frames: 48_000,
        channels: vec![0],
        markers: vec![
            Marker {
                note_id: "a".into(),
                selection_key: 60,
                at_frame: 0,
                gate_end_frame: Some(32_000),
            },
            Marker {
                note_id: "b".into(),
                selection_key: 62,
                at_frame: 12_000,
                gate_end_frame: None,
            },
        ],
    };
    assert_eq!(
        sample.notes(),
        Err("sample gate must end within its region")
    );
}
