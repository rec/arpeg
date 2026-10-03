use arpeg_core::{Beat, HeldNote, render_held};
use serde_json::Value;

fn ratio(text: &str) -> Beat {
    match text.split_once('/') {
        Some((numerator, denominator)) => Beat::new(
            numerator.parse().expect("rational numerator"),
            denominator.parse().expect("rational denominator"),
        ),
        None => Beat::from_integer(text.parse().expect("rational integer")),
    }
}

#[test]
fn held_chord_matches_shared_exact_trace() {
    let case: Value = serde_json::from_str(include_str!("../../../conformance/held-chord.json"))
        .expect("shared fixture");
    let phrase = &case["phrase"];
    let tempo = ratio(case["tempo"]["points"][0]["bpm"].as_str().expect("tempo"));
    let rate = phrase["timebase"]["rate"]["numerator"]
        .as_i64()
        .expect("tick rate");
    let notes: Vec<_> = phrase["notes"]
        .as_array()
        .expect("source notes")
        .iter()
        .map(|note| {
            let at = |field| Beat::new(note[field].as_i64().expect("note tick"), rate) * tempo / 60;
            HeldNote {
                id: note["note_id"].as_str().expect("note identity"),
                key: note["key"].as_i64().expect("note key") as i32,
                onset: at("onset_tick"),
                release: at("gate_end_tick"),
            }
        })
        .collect();
    let step = ratio(
        case["profile"]["body"]["rhythm"]["step"]
            .as_str()
            .expect("grid step")
            .strip_suffix(" beat")
            .expect("beat unit"),
    );
    let through = ratio(case["through"].as_str().expect("render horizon"));
    let occurrences = render_held(&notes, step, Beat::new(4, 5), through).expect("held arp");
    let actual: Vec<_> = occurrences
        .iter()
        .map(|occurrence| {
            vec![
                occurrence.source_id.to_owned(),
                occurrence.onset.to_string(),
                occurrence.gate_end.to_string(),
            ]
        })
        .collect();
    let expected: Vec<Vec<String>> =
        serde_json::from_value(case["expected"].clone()).expect("expected occurrence trace");
    assert_eq!(actual, expected);
    assert_eq!(occurrences.len(), 8);
    assert!(
        occurrences
            .windows(2)
            .all(|window| window[0].trigger_id != window[1].trigger_id)
    );
}
