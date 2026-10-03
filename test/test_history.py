from fractions import Fraction

import pytest
from ufor.events import MidiEvent

from arpeg.history import LiveHistoryArpeggiator


def _event(tick: int, ordinal: int, data: list[int]) -> MidiEvent:
    return MidiEvent(tick=tick, ordinal=ordinal, data=data)


def test_completed_wind_note_plays_on_next_step_with_fitted_breath() -> None:
    arp = LiveHistoryArpeggiator(step=Fraction(100))
    arp.accept(_event(0, 0, [176, 2, 0]))
    arp.accept(_event(0, 1, [144, 60, 100]))
    assert arp.before(8) == []
    arp.accept(_event(8, 0, [176, 2, 80]))
    arp.before(50)
    arp.accept(_event(50, 0, [128, 60, 20]))
    assert [(e.at, e.data) for e in arp.advance(100)] == [
        (100, [176, 2, 0]),
        (100, [144, 60, 100]),
    ]
    assert arp.bank.revision == 1
    assert [(e.at, e.data) for e in arp.advance(113)] == [
        (Fraction(564, 5), [176, 2, 80])
    ]
    assert [(e.at, e.data) for e in arp.advance(180)] == [(180, [128, 60, 20])]


def test_channel_one_handoff_cancels_old_controls_and_releases_owned_note() -> None:
    arp = LiveHistoryArpeggiator(step=Fraction(100), gate=Fraction(2))
    arp.accept(_event(0, 0, [144, 60, 100]))
    arp.before(50)
    arp.accept(_event(50, 0, [128, 60, 0]))
    assert [e.data for e in arp.advance(100)] == [[144, 60, 100]]
    assert [e.data for e in arp.advance(200)] == [
        [128, 60, 0],
        [144, 60, 100],
    ]
    assert [e.data for e in arp.stop(250)] == [[128, 60, 0]]


def test_late_input_after_a_published_step_is_rejected() -> None:
    arp = LiveHistoryArpeggiator(step=Fraction(100))
    arp.advance(0)
    with pytest.raises(ValueError, match="after its live output time"):
        arp.accept(_event(0, 0, [144, 60, 100]))


def test_clear_forgets_old_notes_but_keeps_capturing_new_ones() -> None:
    arp = LiveHistoryArpeggiator(step=Fraction(100))
    arp.accept(_event(0, 0, [144, 60, 100]))
    arp.before(50)
    arp.accept(_event(50, 0, [128, 60, 0]))
    assert [e.data for e in arp.advance(100)] == [[144, 60, 100]]
    assert [e.data for e in arp.clear(150)] == [[128, 60, 0]]
    assert arp.advance(200) == []
    arp.accept(_event(210, 0, [144, 64, 90]))
    arp.before(230)
    arp.accept(_event(230, 0, [128, 64, 0]))
    assert [e.data for e in arp.advance(300)] == [[144, 64, 90]]


def test_reverse_played_order_and_continuity_through_bank_edits() -> None:
    arp = LiveHistoryArpeggiator(
        step=Fraction(100),
        bank={
            "mode": "history",
            "selection": "played",
            "direction": "reverse",
            "retrigger_on_edit": False,
        },
    )
    arp.accept(_event(0, 0, [144, 60, 100]))
    arp.before(10)
    arp.accept(_event(10, 0, [128, 60, 0]))
    arp.before(20)
    arp.accept(_event(20, 0, [144, 64, 100]))
    arp.before(30)
    arp.accept(_event(30, 0, [128, 64, 0]))
    assert [e.data for e in arp.advance(100) if e.data[0] == 144] == [[144, 64, 100]]
    assert [e.data for e in arp.advance(200) if e.data[0] == 144] == [[144, 60, 100]]
    arp.accept(_event(210, 0, [144, 67, 100]))
    arp.before(220)
    arp.accept(_event(220, 0, [128, 67, 0]))
    assert [e.data for e in arp.advance(300) if e.data[0] == 144] == [[144, 67, 100]]
