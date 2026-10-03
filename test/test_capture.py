from pathlib import Path

import pytest
from ufor.arpeggiator_capture import CapturedPhrase
from ufor.events import MidiEvent
from ufor.time import Timebase

from arpeg.capture import MidiCapture, MidiCaptureProfile


@pytest.mark.parametrize("name", ["wind-breath", "gap"])
def test_capture_retains_wind_gestures_and_between_note_events(name: str) -> None:
    source = CapturedPhrase.model_validate_json(
        Path(f"conformance/{name}.json").read_text()
    )
    capture = MidiCapture(
        capture_id=source.capture_id,
        timebase=source.timebase,
        profile=MidiCaptureProfile(
            channel=2 if name == "wind-breath" else 0,
            track_bend=name == "wind-breath",
        ),
    )
    onsets = iter(n.note_id for n in source.notes)
    for event in source.events:
        assert isinstance(event, MidiEvent)
        is_onset = event.data[0] & 0xF0 == 0x90 and event.data[2] > 0
        capture.accept(event, next(onsets) if is_onset else None)
    phrase = capture.finish(source.end_tick)
    assert phrase.events == source.events
    assert phrase.prefix_events == source.prefix_events
    exclude = {"velocity", "release_velocity"}
    assert [n.model_dump(exclude=exclude) for n in phrase.notes] == [
        n.model_dump(exclude=exclude) for n in source.notes
    ]
    if name == "wind-breath":
        assert [n.velocity for n in phrase.notes] == [n.velocity for n in source.notes]
        assert [n.release_velocity for n in phrase.notes] == [
            n.release_velocity for n in source.notes
        ]


def test_legato_handoff_keeps_wire_encoding_and_inherited_breath() -> None:
    timebase = Timebase.model_validate(
        {"name": "milliseconds", "rate": {"numerator": 1000}}
    )
    capture = MidiCapture(capture_id="legato", timebase=timebase)
    for tick, ordinal, data in [
        (0, 0, [176, 2, 20]),
        (0, 1, [144, 60, 100]),
        (15, 0, [176, 2, 80]),
        (30, 0, [144, 64, 90]),
        (50, 0, [144, 64, 0]),
    ]:
        capture.accept(MidiEvent(tick=tick, ordinal=ordinal, data=data))
    phrase = capture.finish(60)
    first, second = phrase.notes
    assert (first.gate_end_tick, first.release_event, first.cell_end_tick) == (
        30,
        None,
        30,
    )
    assert second.entry_state["breath"].source_event == 2
    assert (second.gate_end_tick, second.release_event) == (50, 4)
    assert phrase.events[4].data == [144, 64, 0]


def test_independent_overlap_keeps_distinct_gates_and_shared_observation() -> None:
    timebase = Timebase.model_validate(
        {"name": "milliseconds", "rate": {"numerator": 1000}}
    )
    capture = MidiCapture(
        capture_id="overlap",
        timebase=timebase,
        profile=MidiCaptureProfile(overlap="independent"),
    )
    for tick, ordinal, data in [
        (0, 0, [144, 60, 100]),
        (10, 0, [144, 64, 90]),
        (20, 0, [176, 2, 80]),
        (30, 0, [128, 60, 7]),
        (40, 0, [128, 64, 8]),
    ]:
        capture.accept(MidiEvent(tick=tick, ordinal=ordinal, data=data))
    phrase = capture.finish(50)
    assert [(n.gate_end_tick, n.cell_end_tick) for n in phrase.notes] == [
        (30, 50),
        (40, 50),
    ]
    assert [n.expression_events for n in phrase.notes] == [[2], [2]]
    assert [n.release_velocity for n in phrase.notes] == [7 / 127, 8 / 127]


def test_capture_rejects_ambiguous_time_and_early_end() -> None:
    timebase = Timebase.model_validate(
        {"name": "milliseconds", "rate": {"numerator": 1000}}
    )
    capture = MidiCapture(capture_id="ordered", timebase=timebase)
    capture.accept(MidiEvent(tick=10, ordinal=0, data=[144, 60, 100]))
    with pytest.raises(ValueError, match="increase"):
        capture.accept(MidiEvent(tick=10, ordinal=0, data=[176, 2, 50]))
    with pytest.raises(ValueError, match="precedes"):
        capture.finish(9)


def test_unknown_messages_remain_in_the_ledger() -> None:
    timebase = Timebase.model_validate(
        {"name": "milliseconds", "rate": {"numerator": 1000}}
    )
    capture = MidiCapture(capture_id="opaque", timebase=timebase)
    for tick, data in [
        (0, [192, 7]),
        (10, [144, 60, 90]),
        (20, [240, 1, 247]),
        (30, [128, 60, 0]),
        (40, [176, 64, 127]),
    ]:
        capture.accept(MidiEvent(tick=tick, ordinal=0, data=data))
    phrase = capture.finish(50)
    assert [e.data for e in phrase.events] == [
        [192, 7],
        [144, 60, 90],
        [240, 1, 247],
        [128, 60, 0],
        [176, 64, 127],
    ]
    assert phrase.prefix_events == [0]
    assert phrase.notes[0].expression_events == []
    assert phrase.notes[0].following_events == [4]


def test_completed_note_waits_for_declared_tail_before_history_publication() -> None:
    timebase = Timebase.model_validate(
        {"name": "milliseconds", "rate": {"numerator": 1000}}
    )
    capture = MidiCapture(
        capture_id="history",
        timebase=timebase,
        profile=MidiCaptureProfile(tail_ticks=20),
    )
    capture.accept(MidiEvent(tick=0, ordinal=0, data=[144, 60, 100]))
    capture.accept(MidiEvent(tick=100, ordinal=0, data=[128, 60, 0]))
    capture.accept(MidiEvent(tick=110, ordinal=0, data=[176, 2, 50]))
    assert capture.advance(119) == []
    ready = capture.advance(120)
    assert [(n.gate_end_tick, n.cell_end_tick, n.following_events) for n in ready] == [
        (100, 120, [2])
    ]
    assert capture.advance(130) == []
    assert capture.snapshot(130).notes == ready
    with pytest.raises(ValueError, match="published"):
        capture.accept(MidiEvent(tick=125, ordinal=0, data=[176, 2, 60]))
    with pytest.raises(ValueError, match="precedes"):
        capture.advance(129)
