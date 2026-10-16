from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest
from ufor.arpeggiator import Euclidean, HitStep, Pattern

from arpeg.engine import EngineSettings, TokenArpeggiator
from arpeg.live import LiveArpeggiator
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/finite-cycles.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
@pytest.mark.parametrize("polling", [False, True])
def test_finite_cycles_match_shared_traces_and_snapshots(
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


@pytest.mark.parametrize(
    "rhythm",
    [
        Pattern(cycles=2, steps=[HitStep(duration="1/4 beat")]),
        Euclidean(cycles=2, steps=1, pulses=1, step="1/4 beat"),
    ],
)
def test_transport_and_chord_edits_preserve_the_finite_allowance(
    rhythm: Pattern | Euclidean,
) -> None:
    engine = TokenArpeggiator(settings=EngineSettings(rhythm=rhythm))
    # The first pass is silent. Adding a chord does not rearm that pass.
    engine.advance(Fraction(0))
    engine.press(Fraction(1, 8), 1000, 5000, "a")
    assert [e.value for e in engine.advance(Fraction(1, 4)) if e.kind == "on"] == ["a"]
    engine.release(Fraction(1, 3), 1000)
    engine.press(Fraction(3, 8), 1001, 5001, "b")
    assert [e for e in engine.advance(Fraction(1)) if e.kind == "on"] == []
    assert engine.ports is not None
    assert [e.at for e in engine.ports.take_events().events if e.port == "cycle"] == [
        Fraction(0),
        Fraction(1, 4),
    ]
    engine.relocate(Fraction(3, 4))
    assert engine.advance(Fraction(1)) == []
    engine.relocate(Fraction(0))
    assert [e.value for e in engine.advance(Fraction(0)) if e.kind == "on"] == ["b"]
    engine.pause(Fraction(1))
    assert engine.advance(Fraction(2)) == []
