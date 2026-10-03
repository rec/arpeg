import json
from fractions import Fraction
from pathlib import Path

from ufor.arpeggiator import ArpeggiatorScore
from ufor.arpeggiator_capture import CapturedPhrase, SourceNote
from ufor.control import TempoMap

from arpeg.held import render_held


def test_held_chord_matches_shared_exact_trace() -> None:
    case = json.loads(Path("conformance/held-chord.json").read_text())
    occurrences = render_held(
        ArpeggiatorScore.model_validate(case["profile"]),
        CapturedPhrase.model_validate(case["phrase"]),
        TempoMap.model_validate(case["tempo"]),
        Fraction(case["through"]),
        "synth",
    )
    assert [
        [o.source_note, str(o.onset), str(o.gate_end)] for o in occurrences
    ] == case["expected"]
    assert len({o.trigger_id for o in occurrences}) == len(occurrences)
    assert all(o.destination == "synth" for o in occurrences)


def test_empty_bank_emits_nothing() -> None:
    case = json.loads(Path("conformance/held-chord.json").read_text())
    phrase = CapturedPhrase.model_validate(case["phrase"])
    assert not render_held(
        ArpeggiatorScore.model_validate(case["profile"]),
        phrase.model_copy(update={"notes": []}),
        TempoMap.model_validate(case["tempo"]),
        Fraction(2),
        "synth",
    )


def test_repeated_pitch_keeps_distinct_source_notes() -> None:
    case = json.loads(Path("conformance/held-chord.json").read_text())
    phrase = CapturedPhrase.model_validate(case["phrase"])
    notes = [
        phrase.notes[0],
        phrase.notes[1].model_copy(update={"key": 60}),
        phrase.notes[2],
    ]
    occurrences = render_held(
        ArpeggiatorScore.model_validate(case["profile"]),
        phrase.model_copy(update={"notes": notes}),
        TempoMap.model_validate(case["tempo"]),
        Fraction(1),
        "synth",
    )
    assert [o.source_note for o in occurrences] == ["c", "e", "g", "c"]


def test_last_release_truncates_owned_output_gate() -> None:
    case = json.loads(Path("conformance/held-chord.json").read_text())
    phrase = CapturedPhrase.model_validate(case["phrase"])
    note = SourceNote(
        capture_id="chord",
        note_id="c",
        onset_tick=0,
        gate_end_tick=30,
        cell_end_tick=480,
        key=60,
    )
    occurrences = render_held(
        ArpeggiatorScore.model_validate(case["profile"]),
        phrase.model_copy(update={"notes": [note]}),
        TempoMap.model_validate(case["tempo"]),
        Fraction(1, 4),
        "synth",
    )
    assert len(occurrences) == 1
    assert occurrences[0].gate_end == Fraction(1, 8)
