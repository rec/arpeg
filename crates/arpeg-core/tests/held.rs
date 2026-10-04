use arpeg_core::rhythm::Rhythm;
use arpeg_core::{Bank, Beat, HeldNote, Selection, render_held};
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
fn euclidean_renderer_matches_shared_exact_trace() {
    let notes = ["c", "e", "g"]
        .into_iter()
        .zip([60, 64, 67])
        .map(|(id, key)| HeldNote {
            id,
            key,
            onset: Beat::from_integer(0),
            release: Beat::from_integer(2),
        })
        .collect::<Vec<_>>();
    let occurrences = render_held(
        &notes,
        Bank::Held,
        Selection::Ascending,
        Rhythm::Euclidean {
            step: Beat::new(1, 4),
            steps: 8,
            pulses: 3,
            rotation: 0,
        },
        Beat::new(4, 5),
        Beat::from_integer(2),
    )
    .unwrap();
    let actual: Vec<_> = occurrences
        .iter()
        .map(|o| {
            vec![
                o.source_id.to_owned(),
                o.onset.to_string(),
                o.gate_end.to_string(),
            ]
        })
        .collect();
    let fixture: Value =
        serde_json::from_str(include_str!("../../../conformance/euclidean.json")).unwrap();
    let expected: Vec<Vec<String>> = serde_json::from_value(fixture["rendered"].clone()).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn held_chord_matches_shared_exact_trace() {
    for (fixture, selection, expected_name) in [
        (
            include_str!("../../../conformance/held-chord.json"),
            Selection::Ascending,
            "expected",
        ),
        (
            include_str!("../../../conformance/held-chord.json"),
            Selection::Descending,
            "expected_descending",
        ),
        (
            include_str!("../../../conformance/played-order.json"),
            Selection::Played,
            "expected",
        ),
        (
            include_str!("../../../conformance/played-order.json"),
            Selection::ReversePlayed,
            "expected_reverse",
        ),
        (
            include_str!("../../../conformance/latched-toggle.json"),
            Selection::Ascending,
            "expected",
        ),
    ] {
        let case: Value = serde_json::from_str(fixture).expect("shared fixture");
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
                let at =
                    |field| Beat::new(note[field].as_i64().expect("note tick"), rate) * tempo / 60;
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
        let bank = match case["profile"]["body"]["bank"]["update"].as_str() {
            Some("replace") => Bank::LatchedReplace,
            Some("add") => Bank::LatchedAdd,
            Some("toggle") => Bank::LatchedToggle,
            _ => Bank::Held,
        };
        let occurrences = render_held(
            &notes,
            bank,
            selection,
            Rhythm::Grid { step },
            Beat::new(4, 5),
            through,
        )
        .expect("held arp");
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
            serde_json::from_value(case[expected_name].clone()).expect("expected occurrence trace");
        assert_eq!(actual, expected);
        assert_eq!(occurrences.len(), expected.len());
        assert!(
            occurrences
                .windows(2)
                .all(|window| window[0].trigger_id != window[1].trigger_id)
        );
    }
}

#[test]
fn latch_replace_and_add_keep_their_distinct_banks() {
    let notes = [
        HeldNote {
            id: "c",
            key: 60,
            onset: Beat::from_integer(0),
            release: Beat::from_integer(2),
        },
        HeldNote {
            id: "g",
            key: 67,
            onset: Beat::new(1, 4),
            release: Beat::from_integer(2),
        },
        HeldNote {
            id: "e",
            key: 64,
            onset: Beat::new(1, 2),
            release: Beat::from_integer(2),
        },
    ];
    for (bank, expected) in [
        (Bank::LatchedReplace, ["e", "e"]),
        (Bank::LatchedAdd, ["e", "c"]),
    ] {
        let occurrences = render_held(
            &notes,
            bank,
            Selection::Played,
            Rhythm::Grid {
                step: Beat::new(1, 4),
            },
            Beat::new(4, 5),
            Beat::new(5, 2),
        )
        .expect("latched arp");
        let tail: Vec<_> = occurrences
            .iter()
            .rev()
            .take(2)
            .map(|o| o.source_id)
            .rev()
            .collect();
        assert_eq!(tail, expected);
    }
}
