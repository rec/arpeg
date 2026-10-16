from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest
from ufor.arpeggiator import Grammar

from arpeg.engine import EngineSettings, TokenArpeggiator
from arpeg.live import LiveArpeggiator
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/grammars.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
@pytest.mark.parametrize("polling", [False, True])
def test_grammars_match_shared_traces_and_snapshots(
    case: dict[str, object], polling: bool
) -> None:
    profile = parse_profile(Path("conformance/grammar.toml").read_text())
    profile = type(profile).model_validate(
        profile.model_dump()
        | {
            "body": profile.body.model_dump()
            | {
                "selection": case["selection"],
                "rhythm": case.get("rhythm", {"kind": "grid", "step": "1/4 beat"}),
                "octaves": case.get("octaves", {"offsets": [0]}),
            }
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


def test_generic_grammar_playback_and_snapshots_preserve_opaque_payloads() -> None:
    grammar = Grammar(
        axiom=["A"],
        rules={"A": ["A", "B"], "B": ["A"]},
        terminals={"A": 0, "B": 1},
        rounds=3,
    )
    engine = TokenArpeggiator(settings=EngineSettings(selection=grammar))
    engine.press(Fraction(0), 1000, 0, "λ")
    engine.press(Fraction(0), 1001, 1, "猫")
    engine.advance(Fraction(1, 4))
    restored = TokenArpeggiator.model_validate_json(engine.model_dump_json())
    events = restored.advance(Fraction(5, 4))
    assert events == engine.advance(Fraction(5, 4))
    assert [e.value for e in events if e.kind == "on"] == ["λ", "λ", "猫", "λ"]
