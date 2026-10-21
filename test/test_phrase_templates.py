from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest
from ufor.arpeggiator import ArpeggiatorScore, IndexPattern

from arpeg.engine import EngineSettings, TokenArpeggiator
from arpeg.live import LiveArpeggiator
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/phrase-templates.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
@pytest.mark.parametrize("bank", ["held", "latched"])
@pytest.mark.parametrize("polling", [False, True])
def test_phrase_templates_substitute_chord_notes_and_match_shared_traces(
    case: dict[str, object], bank: str, polling: bool
) -> None:
    engine = LiveArpeggiator(
        profile=_profile(
            bank={"kind": bank},
            selection=case["selection"],
            octaves=case.get("octaves", {"offsets": [0]}),
            patterns=case.get("patterns"),
        )
    )
    for i, key in enumerate([60, 64, 67]):
        engine.note_on(Fraction(0), key, 90 - i * 10)
    events = []
    through = Fraction(49, 50)
    while polling and engine.now + Fraction(1, 17) < through:
        events.extend(engine.advance(engine.now + Fraction(1, 17)))
        engine = LiveArpeggiator.model_validate_json(engine.model_dump_json())
    events.extend(engine.advance(through))
    starts = [e for e in events if e.kind == "on"]
    assert [e.key for e in starts] == case.get("held_keys", case["keys"])
    assert [e.at for e in starts] == (
        [Fraction(b) for b in case["held_beats"]]
        if "held_beats" in case
        else [Fraction(b) - Fraction(1, 4) for b in case["beats"]]
    )
    assert [e.velocity for e in starts] == [90 - e.source_id * 10 for e in starts]
    assert [(e.id, e.key) for e in starts] == [
        (e.id, e.key) for e in events if e.kind == "off"
    ]


def test_repeats_and_ties_retain_their_interval_after_chord_edits() -> None:
    engine = LiveArpeggiator(
        profile=_profile(
            selection={
                "kind": "index_pattern",
                "indices": [0, 1],
                "intervals": [12, -12],
            },
            rhythm={
                "kind": "pattern",
                "steps": [
                    {"kind": "hit", "duration": "1/2 beat", "repeats": 2},
                    {"kind": "tie", "duration": "1/4 beat"},
                ],
            },
        )
    )
    engine.note_on(Fraction(0), 60, 90)
    engine.note_on(Fraction(0), 64, 80)
    events = engine.advance(Fraction(0))
    events.extend(engine.note_on(Fraction(1, 8), 48, 70))
    engine = LiveArpeggiator.model_validate_json(engine.model_dump_json())
    events.extend(engine.advance(Fraction(1)))
    assert [(e.at, e.key, e.velocity) for e in events if e.kind == "on"] == [
        (Fraction(0), 72, 90),
        (Fraction(1, 4), 72, 90),
        (Fraction(3, 4), 48, 90),
        (Fraction(1), 48, 90),
    ]
    assert [(e.at, e.key) for e in events if e.kind == "off"] == [
        (Fraction(1, 5), 72),
        (Fraction(7, 10), 72),
        (Fraction(19, 20), 48),
    ]


def test_intervals_precede_scale_mapping_and_global_transposition() -> None:
    engine = LiveArpeggiator(
        profile=_profile(
            selection={"kind": "index_pattern", "indices": [0], "intervals": [4]},
            scale={"definition": {"notes": "CDEFGAB"}, "degrees": 1},
            transposition={"semitones": 12},
        )
    )
    engine.note_on(Fraction(0), 60, 90)
    assert [e.key for e in engine.advance(Fraction(0))] == [77]


def test_intervals_count_tuning_grid_steps() -> None:
    engine = LiveArpeggiator(
        profile=_profile(
            selection={"kind": "index_pattern", "indices": [0], "intervals": [19]},
            tuning={
                "source": {
                    "kind": "computed",
                    "notes_per_octave": 19,
                    "octave_ratio": "2",
                },
                "root_note": 69,
                "root_frequency": "440",
            },
            bend_range=2,
        )
    )
    engine.note_on(Fraction(0), 69, 90)
    assert [e.key for e in engine.advance(Fraction(0))] == [81]


def test_nonzero_pitch_intervals_require_a_musical_destination() -> None:
    engine = TokenArpeggiator(
        settings=EngineSettings(selection=IndexPattern(indices=[0], intervals=[12]))
    )
    engine.press(Fraction(0), 1, 1, "word")
    with pytest.raises(ValueError, match="musical destination"):
        engine.advance(Fraction(0))


def _profile(**changes: object) -> ArpeggiatorScore:
    profile = parse_profile(Path("conformance/up.toml").read_text())
    return ArpeggiatorScore.model_validate(
        profile.model_dump() | {"body": profile.body.model_dump() | changes}
    )
