use arpeg_core::chance::draw_below;

#[test]
fn named_draws_match_shared_vectors() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../conformance/chance-walk.json")).unwrap();
    for (lane, bound) in [("probability", 3), ("walk", 5)] {
        let actual: Vec<u64> = (0..12)
            .map(|decision| draw_below(42, "weighted-walk", lane, 3, decision, bound))
            .collect();
        let expected: Vec<u64> = fixture["draws"][lane]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap())
            .collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn single_outcome_always_returns_zero() {
    assert_eq!(draw_below(-42, "chord", "walk", 7, 99, 1), 0);
}
