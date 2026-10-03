use arpeg_core::{
    Beat, Selection,
    live::{LiveArpeggiator, OutputEvent, OutputKind},
};

fn beat(n: i64, d: i64) -> Beat {
    Beat::new(n, d)
}

#[test]
fn live_notes_follow_input_and_release_when_bank_empties() {
    let mut arp = LiveArpeggiator::new(Selection::Ascending, beat(1, 4), beat(4, 5)).unwrap();
    assert_eq!(arp.note_on(beat(0, 1), 60, 100).unwrap(), []);
    assert_eq!(
        arp.advance(beat(0, 1)).unwrap(),
        [OutputEvent {
            at: beat(0, 1),
            kind: OutputKind::NoteOn {
                id: 0,
                key: 60,
                velocity: 100
            }
        }]
    );
    assert_eq!(arp.note_on(beat(1, 8), 64, 90).unwrap(), []);
    assert_eq!(
        arp.advance(beat(1, 4)).unwrap(),
        [
            OutputEvent {
                at: beat(1, 5),
                kind: OutputKind::NoteOff { id: 0, key: 60 }
            },
            OutputEvent {
                at: beat(1, 4),
                kind: OutputKind::NoteOn {
                    id: 1,
                    key: 64,
                    velocity: 90
                }
            },
        ]
    );
    assert_eq!(arp.note_off(beat(3, 10), 60).unwrap(), []);
    assert_eq!(
        arp.note_off(beat(7, 20), 64).unwrap(),
        [OutputEvent {
            at: beat(7, 20),
            kind: OutputKind::NoteOff { id: 1, key: 64 }
        }]
    );
    assert_eq!(arp.advance(beat(1, 2)).unwrap(), []);
}

#[test]
fn live_step_order_is_independent_of_poll_intervals() {
    let mut a = LiveArpeggiator::new(Selection::Ascending, beat(1, 4), beat(1, 1)).unwrap();
    let mut b = LiveArpeggiator::new(Selection::Ascending, beat(1, 4), beat(1, 1)).unwrap();
    a.note_on(beat(0, 1), 60, 100).unwrap();
    b.note_on(beat(0, 1), 60, 100).unwrap();
    a.advance(beat(0, 1)).unwrap();
    b.advance(beat(0, 1)).unwrap();
    let one = a.advance(beat(1, 1)).unwrap();
    let mut pieces = Vec::new();
    for i in 1..=8 {
        pieces.extend(b.advance(beat(i, 8)).unwrap());
    }
    assert_eq!(one, pieces);
}

#[test]
fn simultaneous_note_ons_join_the_first_step() {
    let mut arp = LiveArpeggiator::new(Selection::Ascending, beat(1, 4), beat(1, 1)).unwrap();
    arp.note_on(beat(0, 1), 64, 90).unwrap();
    arp.note_on(beat(0, 1), 60, 100).unwrap();
    assert_eq!(
        arp.advance(beat(0, 1)).unwrap(),
        [OutputEvent {
            at: beat(0, 1),
            kind: OutputKind::NoteOn {
                id: 0,
                key: 60,
                velocity: 100
            }
        }]
    );
}
