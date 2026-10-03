from ufor.events import MidiEvent
from ufor.time import Timebase

from arpeg.bank import BankNote, CaptureBank
from arpeg.capture import MidiCaptureProfile
from arpeg.gesture import MidiGestureRenderer, MidiPlacement


def _timebase() -> Timebase:
    return Timebase.model_validate(
        {"name": "milliseconds", "rate": {"numerator": 1000}}
    )


def _note(bank: CaptureBank, key: int, start: int, note_id: str) -> None:
    bank.accept(MidiEvent(tick=start, ordinal=0, data=[144, key, 90]), note_id=note_id)
    bank.accept(MidiEvent(tick=start + 50, ordinal=0, data=[128, key, 0]))


def test_history_publishes_completed_notes_on_steps_with_a_bounded_bank() -> None:
    bank = CaptureBank(mode="history", history_size=1)
    bank.record("take", _timebase(), MidiCaptureProfile(tail_ticks=20))
    _note(bank, 60, 0, "c")
    bank.advance(69)
    assert bank.publish_step() == []
    bank.advance(70)
    assert bank.publish_step() == [BankNote(capture_id="take", note_id="c")]
    assert bank.revision == 1
    _note(bank, 64, 100, "e")
    bank.advance(170)
    assert bank.publish_step() == [BankNote(capture_id="take", note_id="e")]
    assert bank.revision == 2
    assert [n.note_id for n in bank.source("take").notes] == ["c", "e"]
    bank.commit(180)
    assert bank.publish_step() == [BankNote(capture_id="take", note_id="e")]
    assert bank.revision == 2

    bank.record("next", _timebase(), MidiCaptureProfile())
    _note(bank, 67, 0, "g")
    bank.commit(60)
    assert bank.publish_step() == [BankNote(capture_id="next", note_id="g")]


def test_phrase_replace_overdub_undo_and_clear_publish_revisions() -> None:
    bank = CaptureBank(mode="phrase")
    bank.record("first", _timebase(), MidiCaptureProfile())
    _note(bank, 60, 0, "c")
    bank.commit(60)
    assert bank.published == []
    assert bank.publish_step() == [BankNote(capture_id="first", note_id="c")]

    bank.record("second", _timebase(), MidiCaptureProfile())
    _note(bank, 64, 0, "e")
    bank.commit(60, update="overdub")
    assert bank.publish_step() == [
        BankNote(capture_id="first", note_id="c"),
        BankNote(capture_id="second", note_id="e"),
    ]
    bank.undo_last_capture()
    assert bank.publish_step() == [BankNote(capture_id="first", note_id="c")]

    bank.record("replacement", _timebase(), MidiCaptureProfile())
    _note(bank, 67, 0, "g")
    bank.commit(60, update="replace")
    assert bank.publish_step() == [BankNote(capture_id="replacement", note_id="g")]
    bank.undo_last_capture()
    assert bank.publish_step() == [BankNote(capture_id="first", note_id="c")]
    bank.clear()
    assert bank.publish_step() == []


def test_live_history_step_selects_and_plays_a_completed_gesture() -> None:
    bank = CaptureBank(mode="history")
    bank.record("wind", _timebase(), MidiCaptureProfile(tail_ticks=10))
    bank.accept(MidiEvent(tick=0, ordinal=0, data=[176, 2, 0]))
    bank.accept(MidiEvent(tick=0, ordinal=1, data=[144, 60, 90]), note_id="c")
    bank.accept(MidiEvent(tick=8, ordinal=0, data=[176, 2, 80]))
    bank.accept(MidiEvent(tick=100, ordinal=0, data=[128, 60, 10]))
    bank.advance(109)
    assert bank.select_step() is None
    bank.advance(110)
    selected = bank.select_step()
    assert selected == BankNote(capture_id="wind", note_id="c")
    phrase = bank.source(selected.capture_id)
    events = MidiGestureRenderer(phrase=phrase, channels=[0]).render(
        [MidiPlacement(note_id=selected.note_id, onset=200)]
    )
    assert [(e.at, e.data) for e in events] == [
        (200, [176, 2, 0]),
        (200, [144, 60, 90]),
        (208, [176, 2, 80]),
        (300, [128, 60, 10]),
    ]
