import json
from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest
from ufor.arpeggiator import ArpeggiatorScore, Grid
from ufor.arpeggiator_capture import CapturedPhrase
from ufor.control import TempoMap

from arpeg.engine import EngineSettings, TokenArpeggiator
from arpeg.held import render_held
from arpeg.live import LiveArpeggiator
from arpeg.midi import MidiPlayer, decode_message
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/groove.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
@pytest.mark.parametrize("polling", [False, True])
def test_grooves_match_shared_traces_and_snapshots(
    case: dict[str, object], polling: bool
) -> None:
    profile = parse_profile(Path("conformance/up.toml").read_text())
    profile = type(profile).model_validate(
        profile.model_dump()
        | {
            "body": profile.body.model_dump()
            | {"rhythm": case["rhythm"], "patterns": case.get("patterns")}
        }
    )
    engine = LiveArpeggiator(profile=profile)
    for key, velocity in case["notes"]:
        engine.note_on(Fraction(0), key, velocity)
    events = []
    through = Fraction(case["through"])
    while polling and engine.now + Fraction(1, 17) < through:
        events.extend(engine.advance(engine.now + Fraction(1, 17)))
        engine = LiveArpeggiator.model_validate_json(engine.model_dump_json())
    events.extend(engine.advance(through))
    assert [[str(e.at), e.kind, e.key, e.velocity] for e in events] == case["events"]


@pytest.mark.parametrize("command", ["relocate", "pause"])
@pytest.mark.parametrize(
    ("at", "start"),
    [("1/4", "3/8"), ("3/8", "3/8"), ("2/5", "1/2"), ("4/5", "1"), ("25/4", "51/8")],
)
def test_transport_finds_the_next_grooved_start(
    command: str, at: str, start: str
) -> None:
    engine = TokenArpeggiator(
        settings=EngineSettings(rhythm=Grid(step="1/4 beat", groove=[3, 1, 2, 2]))
    )
    engine.press(Fraction(0), 1000, 5000, "a")
    getattr(engine, command)(Fraction(at))
    engine = TokenArpeggiator.model_validate_json(engine.model_dump_json())
    assert [
        (e.at, e.value) for e in engine.advance(Fraction(start)) if e.kind == "on"
    ] == [(Fraction(start), "a")]


def test_empty_banks_and_chord_edits_preserve_groove_phase() -> None:
    engine = TokenArpeggiator(
        settings=EngineSettings(rhythm=Grid(step="1/4 beat", groove=[3, 1, 2, 2]))
    )
    engine.advance(Fraction(0))
    engine.press(Fraction(1, 8), 1000, 5000, "a")
    assert [e.at for e in engine.advance(Fraction(3, 8)) if e.kind == "on"] == [
        Fraction(3, 8)
    ]
    engine.release(Fraction(2, 5), 1000)
    engine.press(Fraction(9, 20), 1001, 5001, "b")
    assert [
        (e.at, e.value) for e in engine.advance(Fraction(1, 2)) if e.kind == "on"
    ] == [(Fraction(1, 2), "b")]


def test_file_rendering_includes_an_early_start_before_the_horizon() -> None:
    case = json.loads(Path("conformance/held-chord.json").read_text())
    case["profile"]["body"]["rhythm"] = {
        "kind": "grid",
        "step": "1/4 beat",
        "groove": [1, 3],
    }
    profile = ArpeggiatorScore.model_validate(case["profile"])
    events = render_held(
        profile,
        CapturedPhrase.model_validate(case["phrase"]),
        TempoMap.model_validate(case["tempo"]),
        Fraction(1, 5),
        "synth",
    )
    assert [e.onset for e in events] == [Fraction(0), Fraction(1, 8)]


def test_captured_gestures_fit_each_grooved_interval() -> None:
    profile = parse_profile(Path("conformance/history-wind.toml").read_text())
    profile = type(profile).model_validate(
        profile.model_dump()
        | {
            "body": profile.body.model_dump()
            | {"rhythm": {"kind": "grid", "step": "1/4 beat", "groove": [3, 1, 2, 2]}}
        }
    )
    player = MidiPlayer(profile=profile)
    player.accept(0, decode_message([144, 60, 100]))
    player.accept(50_000_000, decode_message([128, 60, 20]))
    player.advance(500_000_000)
    assert [(e.port.value, e.at) for e in player.take_events().notes] == [
        ("note_start", Fraction(3, 8)),
        ("note_end", Fraction(19, 40)),
        ("note_start", Fraction(1, 2)),
        ("note_end", Fraction(7, 10)),
        ("note_start", Fraction(3, 4)),
        ("note_end", Fraction(19, 20)),
        ("note_start", Fraction(1)),
    ]
