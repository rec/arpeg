use arpeg_core::{
    Selection,
    capture::{MidiEvent, Profile},
    gesture::Tick,
    history::HistoryArpeggiator,
};

fn event(tick: i64, ordinal: u32, data: &[u8]) -> MidiEvent {
    MidiEvent {
        tick,
        ordinal,
        data: data.to_vec(),
    }
}

#[test]
fn completed_wind_note_plays_on_next_step_with_fitted_breath() {
    let mut arp = HistoryArpeggiator::new(
        8,
        Selection::Ascending,
        true,
        Tick::new(1, 4),
        Tick::new(4, 5),
        Profile::default(),
        false,
    )
    .unwrap();
    arp.accept(event(0, 0, &[176, 2, 0])).unwrap();
    arp.accept(event(0, 1, &[144, 60, 100])).unwrap();
    assert!(arp.before(Tick::new(8, 400), 8).unwrap().is_empty());
    arp.accept(event(8, 0, &[176, 2, 80])).unwrap();
    arp.before(Tick::new(50, 400), 50).unwrap();
    arp.accept(event(50, 0, &[128, 60, 20])).unwrap();
    let output = arp.advance(Tick::new(100, 400), 100).unwrap();
    assert_eq!(
        output
            .iter()
            .map(|event| (event.at, event.data.clone()))
            .collect::<Vec<_>>(),
        [
            (Tick::new(1, 4), vec![176, 2, 0]),
            (Tick::new(1, 4), vec![144, 60, 100]),
        ]
    );
    assert_eq!(arp.bank_revision, 1);
    let expression = arp.advance(Tick::new(113, 400), 113).unwrap();
    assert_eq!(expression.len(), 1);
    assert_eq!(expression[0].at, Tick::new(141, 500));
    assert_eq!(expression[0].data, [176, 2, 80]);
    assert_eq!(
        arp.advance(Tick::new(180, 400), 180)
            .unwrap()
            .iter()
            .map(|event| event.data.clone())
            .collect::<Vec<_>>(),
        [vec![128, 60, 20]]
    );
}

#[test]
fn channel_one_handoff_cancels_old_controls_and_releases_owned_note() {
    let mut arp = HistoryArpeggiator::new(
        8,
        Selection::Ascending,
        true,
        Tick::new(1, 4),
        Tick::from_integer(2),
        Profile::default(),
        false,
    )
    .unwrap();
    arp.accept(event(0, 0, &[144, 60, 100])).unwrap();
    arp.before(Tick::new(50, 400), 50).unwrap();
    arp.accept(event(50, 0, &[128, 60, 0])).unwrap();
    assert_eq!(
        arp.advance(Tick::new(100, 400), 100)
            .unwrap()
            .iter()
            .map(|event| event.data.clone())
            .collect::<Vec<_>>(),
        [vec![144, 60, 100]]
    );
    let at_200 = arp.advance(Tick::new(200, 400), 200).unwrap();
    assert_eq!(
        at_200
            .iter()
            .map(|event| event.data.clone())
            .collect::<Vec<_>>(),
        [vec![128, 60, 0], vec![144, 60, 100]]
    );
    assert_eq!(
        arp.clear(Tick::new(250, 400), 250)
            .unwrap()
            .iter()
            .map(|event| event.data.clone())
            .collect::<Vec<_>>(),
        [vec![128, 60, 0]]
    );
    assert!(arp.advance(Tick::new(350, 400), 350).unwrap().is_empty());
}

#[test]
fn late_input_after_a_published_step_is_rejected() {
    let mut arp = HistoryArpeggiator::new(
        8,
        Selection::Ascending,
        true,
        Tick::new(1, 4),
        Tick::from_integer(1),
        Profile::default(),
        false,
    )
    .unwrap();
    arp.advance(Tick::new(0, 400), 0).unwrap();
    assert!(arp.accept(event(0, 0, &[144, 60, 100])).is_err());
}

#[test]
fn clear_forgets_old_notes_but_keeps_capturing_new_ones() {
    let mut arp = HistoryArpeggiator::new(
        8,
        Selection::Ascending,
        true,
        Tick::new(1, 4),
        Tick::new(4, 5),
        Profile::default(),
        false,
    )
    .unwrap();
    arp.accept(event(0, 0, &[144, 60, 100])).unwrap();
    arp.before(Tick::new(50, 400), 50).unwrap();
    arp.accept(event(50, 0, &[128, 60, 0])).unwrap();
    assert_eq!(
        arp.advance(Tick::new(100, 400), 100).unwrap()[0].data,
        [144, 60, 100]
    );
    assert_eq!(
        arp.clear(Tick::new(150, 400), 150).unwrap()[0].data,
        [128, 60, 0]
    );
    assert!(arp.advance(Tick::new(200, 400), 200).unwrap().is_empty());
    arp.accept(event(210, 0, &[144, 64, 90])).unwrap();
    arp.before(Tick::new(230, 400), 230).unwrap();
    arp.accept(event(230, 0, &[128, 64, 0])).unwrap();
    assert_eq!(
        arp.advance(Tick::new(300, 400), 300).unwrap()[0].data,
        [144, 64, 90]
    );
}
