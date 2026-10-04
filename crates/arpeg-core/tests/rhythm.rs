use arpeg_core::{Beat, rhythm::Rhythm};
use serde_json::Value;

#[test]
fn euclidean_rotations_match_canonical_masks() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../../conformance/euclidean.json")).unwrap();
    for rotation in -8_i64..16 {
        let rhythm = Rhythm::Euclidean {
            step: Beat::new(1, 4),
            steps: 8,
            pulses: 3,
            rotation,
        };
        let actual: String = (0..16)
            .map(|i| if rhythm.allows_step(i) { '1' } else { '0' })
            .collect();
        let expected = fixture["masks"][rotation.rem_euclid(8) as usize]
            .as_str()
            .unwrap()
            .repeat(2);
        assert_eq!(actual, expected);
    }
}

#[test]
fn euclidean_pulse_counts_and_spacing() {
    for steps in 1..=16 {
        for pulses in 0..=steps {
            for rotation in 0..steps {
                let rhythm = Rhythm::Euclidean {
                    step: Beat::new(1, 3),
                    steps,
                    pulses,
                    rotation,
                };
                let hits: Vec<_> = (0..steps).filter(|i| rhythm.allows_step(*i)).collect();
                assert_eq!(hits.len() as i64, pulses);
                if !hits.is_empty() {
                    let gaps: Vec<_> = hits
                        .iter()
                        .zip(hits.iter().skip(1).copied().chain([hits[0] + steps]))
                        .map(|(a, b)| b - a)
                        .collect();
                    assert!(gaps.iter().max().unwrap() - gaps.iter().min().unwrap() <= 1);
                }
            }
        }
    }
}

#[test]
fn invalid_euclidean_masks_are_rejected_before_playback() {
    for (steps, pulses) in [(0, 0), (-1, 0), (8, -1), (8, 9)] {
        let rhythm = Rhythm::Euclidean {
            step: Beat::new(1, 4),
            steps,
            pulses,
            rotation: 0,
        };
        assert!(rhythm.validate().is_err());
    }
}
