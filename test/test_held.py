import json
from fractions import Fraction
from pathlib import Path

import pytest
from ufor.arpeggiator import ArpeggiatorScore
from ufor.arpeggiator_capture import CapturedPhrase, SourceNote
from ufor.codec import parse_score
from ufor.control import TempoMap

from arpeg.held import render_held


def test_euclidean_renderer_matches_shared_exact_trace() -> None:
    chord = json.loads(Path("conformance/held-chord.json").read_text())
    profile = ArpeggiatorScore.model_validate(
        parse_score(Path("conformance/euclidean.toml").read_text())
    )
    occurrences = render_held(
        profile,
        CapturedPhrase.model_validate(chord["phrase"]),
        TempoMap.model_validate(chord["tempo"]),
        Fraction(2),
        "synth",
    )
    expected = json.loads(Path("conformance/euclidean.json").read_text())["rendered"]
    assert [
        [o.source_note, str(o.onset), str(o.gate_end)] for o in occurrences
    ] == expected


def test_custom_steps_require_live_input() -> None:
    chord = json.loads(Path("conformance/held-chord.json").read_text())
    profile = ArpeggiatorScore.model_validate(
        parse_score(Path("conformance/custom-steps.toml").read_text())
    )
    with pytest.raises(ValueError, match="grid or Euclidean"):
        render_held(
            profile,
            CapturedPhrase.model_validate(chord["phrase"]),
            TempoMap.model_validate(chord["tempo"]),
            Fraction(2),
            "synth",
        )


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


@pytest.mark.parametrize(
    ("name", "selection", "expected"),
    [
        ("held-chord", "descending", "expected_descending"),
        ("played-order", "played", "expected"),
        ("played-order", "reverse", "expected_reverse"),
    ],
)
def test_classic_orders_match_shared_traces(
    name: str, selection: str, expected: str
) -> None:
    case = json.loads(Path(f"conformance/{name}.json").read_text())
    case["profile"]["body"]["selection"] = (
        {"kind": "played", "direction": "reverse"}
        if selection == "reverse"
        else {"kind": selection}
    )
    occurrences = render_held(
        ArpeggiatorScore.model_validate(case["profile"]),
        CapturedPhrase.model_validate(case["phrase"]),
        TempoMap.model_validate(case["tempo"]),
        Fraction(case["through"]),
        "synth",
    )
    assert [
        [o.source_note, str(o.onset), str(o.gate_end)] for o in occurrences
    ] == case[expected]


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


def test_latch_toggle_by_pitch_matches_shared_trace() -> None:
    case = json.loads(Path("conformance/latched-toggle.json").read_text())
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


@pytest.mark.parametrize(
    ("update", "expected"), [("replace", ["e", "e"]), ("add", ["e", "c"])]
)
def test_latch_survives_source_releases(update: str, expected: list[str]) -> None:
    case = json.loads(Path("conformance/played-order.json").read_text())
    case["profile"]["body"]["bank"] = {"kind": "latched", "update": update}
    occurrences = render_held(
        ArpeggiatorScore.model_validate(case["profile"]),
        CapturedPhrase.model_validate(case["phrase"]),
        TempoMap.model_validate(case["tempo"]),
        Fraction(5, 2),
        "synth",
    )
    assert [o.source_note for o in occurrences][-2:] == expected


def test_file_renderer_rejects_live_retrigger_policy() -> None:
    case = json.loads(Path("conformance/held-chord.json").read_text())
    case["profile"]["body"]["retrigger"] = "bank_edit"
    with pytest.raises(ValueError, match="bank-edit retrigger"):
        render_held(
            ArpeggiatorScore.model_validate(case["profile"]),
            CapturedPhrase.model_validate(case["phrase"]),
            TempoMap.model_validate(case["tempo"]),
            Fraction(case["through"]),
            "synth",
        )
