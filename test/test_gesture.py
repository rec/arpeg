from fractions import Fraction
from pathlib import Path

import pytest
from ufor.arpeggiator_capture import CapturedPhrase
from ufor.events import MidiEvent

from arpeg.capture import MidiCaptureProfile
from arpeg.gesture import MidiGestureRenderer, MidiPlacement


def _wind() -> CapturedPhrase:
    return CapturedPhrase.model_validate_json(
        Path("conformance/wind-breath.json").read_text()
    )


def test_reordered_wind_notes_restore_entry_state_and_local_attack() -> None:
    renderer = MidiGestureRenderer(phrase=_wind(), channels=[0])
    events = renderer.reorder(["e", "c"])
    assert [(e.at, e.data, e.source_event) for e in events] == [
        (0, [224, 64, 81], 4),
        (0, [176, 2, 13], 6),
        (0, [144, 64, 90], 7),
        (15, [176, 2, 83], 8),
        (180, [128, 64, 16], 9),
        (180, [176, 2, 0], 0),
        (180, [144, 60, 100], 1),
        (188, [176, 2, 38], 2),
        (220, [176, 2, 102], 3),
        (270, [224, 64, 81], 4),
        (360, [128, 60, 20], 5),
    ]
    assert all(e.source_event != 6 or e.source_note == "e" for e in events)


def test_overlap_requires_an_independent_output_channel() -> None:
    placements = [
        MidiPlacement(note_id="c", onset=Fraction(0)),
        MidiPlacement(note_id="e", onset=Fraction(50)),
    ]
    with pytest.raises(ValueError, match="no MIDI channel"):
        MidiGestureRenderer(phrase=_wind(), channels=[0]).render(placements)
    events = MidiGestureRenderer(phrase=_wind(), channels=[0, 1]).render(placements)
    assert [(e.at, e.data) for e in events if e.data[0] & 0xF0 == 0x90] == [
        (0, [144, 60, 100]),
        (50, [145, 64, 90]),
    ]
    assert (65, [177, 2, 83]) in [(e.at, e.data) for e in events]


def test_monophonic_channel_handoff_releases_before_new_expression() -> None:
    renderer = MidiGestureRenderer(phrase=_wind(), channels=[0], overlap="handoff")
    events = renderer.render(
        [
            MidiPlacement(note_id="c", onset=Fraction(0)),
            MidiPlacement(note_id="e", onset=Fraction(50)),
        ]
    )
    assert [(e.data, e.source_event) for e in events if e.at == 50] == [
        ([128, 60, 0], None),
        ([224, 64, 81], 4),
        ([176, 2, 13], 6),
        ([144, 64, 90], 7),
    ]
    assert all(e.source_event != 4 or e.source_note == "e" for e in events)


def test_fit_scales_recorded_expression_with_gate() -> None:
    renderer = MidiGestureRenderer(phrase=_wind(), channels=[0], timing="fit")
    events = renderer.render(
        [MidiPlacement(note_id="c", onset=Fraction(10), gate=Fraction(90))]
    )
    assert [(e.at, e.data) for e in events] == [
        (10, [176, 2, 0]),
        (10, [144, 60, 100]),
        (14, [176, 2, 38]),
        (30, [176, 2, 102]),
        (55, [224, 64, 81]),
        (100, [128, 60, 20]),
    ]


def test_reorder_carries_cell_gap_without_emitting_unowned_controller() -> None:
    renderer = MidiGestureRenderer(
        phrase=CapturedPhrase.model_validate_json(
            Path("conformance/gap.json").read_text()
        ),
        channels=[0],
    )
    events = renderer.reorder(["c", "e"])
    assert [(e.at, e.data) for e in events] == [
        (0, [144, 60, 90]),
        (90, [128, 60, 0]),
        (190, [176, 2, 13]),
        (190, [144, 64, 90]),
        (290, [128, 64, 0]),
    ]
    assert all(e.source_event not in (0, 6) for e in events)


def test_current_expression_uses_live_state_instead_of_recorded_gestures() -> None:
    renderer = MidiGestureRenderer(
        phrase=_wind(),
        channels=[0],
        expression_source="current",
        live_profile=MidiCaptureProfile(channel=2),
        live_events=[
            MidiEvent(tick=0, ordinal=0, data=[178, 2, 64]),
            MidiEvent(tick=20, ordinal=0, data=[178, 2, 64]),
            MidiEvent(tick=20, ordinal=1, data=[178, 2, 64]),
            MidiEvent(tick=50, ordinal=0, data=[226, 64, 72]),
            MidiEvent(tick=50, ordinal=1, data=[210, 14]),
            MidiEvent(tick=200, ordinal=0, data=[178, 2, 100]),
        ],
    )
    events = renderer.reorder(["e", "c"])
    assert [(e.at, e.data) for e in events] == [
        (0, [176, 2, 64]),
        (0, [144, 64, 90]),
        (20, [176, 2, 64]),
        (20, [176, 2, 64]),
        (50, [224, 64, 72]),
        (50, [208, 14]),
        (180, [128, 64, 16]),
        (180, [176, 2, 64]),
        (180, [224, 64, 72]),
        (180, [208, 14]),
        (180, [144, 60, 100]),
        (200, [176, 2, 100]),
        (360, [128, 60, 20]),
    ]
    assert all(e.source_event not in (2, 3, 4, 6, 8) for e in events if e.at > 0)


@pytest.mark.parametrize("name", ["wind-breath", "gap"])
def test_original_phrase_replay_preserves_every_wire_event(name: str) -> None:
    phrase = CapturedPhrase.model_validate_json(
        Path(f"conformance/{name}.json").read_text()
    )
    events = MidiGestureRenderer(phrase=phrase, channels=[0]).replay_original()
    assert [(e.at, e.data, e.source_event) for e in events] == [
        (event.tick, event.data, index)
        for index, event in enumerate(phrase.events)
        if isinstance(event, MidiEvent)
    ]
