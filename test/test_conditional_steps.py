from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest
from ufor.arpeggiator import Grid, HitStep, PassCondition, Pattern, StepPatterns

from arpeg.engine import EngineSettings, TokenArpeggiator
from arpeg.live import LiveArpeggiator
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/conditional-steps.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
@pytest.mark.parametrize("polling", [False, True])
def test_conditional_steps_match_shared_traces_and_snapshots(
    case: dict[str, object], polling: bool
) -> None:
    profile = parse_profile(Path("conformance/up.toml").read_text())
    profile = type(profile).model_validate(
        profile.model_dump()
        | {"body": profile.body.model_dump() | {"rhythm": case["rhythm"]}}
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


def test_skipped_conditions_preserve_the_audible_chance_sequence() -> None:
    conditional = Pattern(
        steps=[
            HitStep(duration="1/4 beat", condition=PassCondition(passes=[2], out_of=2))
        ]
    )
    starts = []
    for rhythm in [conditional, Grid(step="1/2 beat")]:
        engine = TokenArpeggiator(
            settings=EngineSettings(rhythm=rhythm, seed=42, probability=Fraction(1, 2))
        )
        engine.press(Fraction(0), 1000, 5000, "a")
        engine.press(Fraction(0), 1001, 5001, "b")
        starts.append(
            [(e.at, e.value) for e in engine.advance(Fraction(63, 4)) if e.kind == "on"]
        )
    assert starts[0]
    assert starts[0] == [(a + Fraction(1, 4), v) for a, v in starts[1]]


@pytest.mark.parametrize("command", ["relocate", "pause"])
def test_transport_uses_destination_pass_and_silent_passes_keep_phase(
    command: str,
) -> None:
    engine = TokenArpeggiator(
        settings=EngineSettings(
            rhythm=Pattern(
                steps=[
                    HitStep(
                        duration="1/4 beat",
                        condition=PassCondition(passes=[4], out_of=4),
                    )
                ]
            ),
            patterns=StepPatterns(accents=[Fraction(1), Fraction(2)]),
        )
    )
    engine.advance(Fraction(1, 4))
    engine.press(Fraction(1, 3), 1000, 5000, "a")
    getattr(engine, command)(Fraction(2, 3))
    events = engine.advance(Fraction(3, 4))
    assert [(e.at, e.kind, e.value, e.accent) for e in events] == [
        (Fraction(3, 4), "on", "a", Fraction(2))
    ]
    engine.release(Fraction(4, 5), 1000)
    engine.press(Fraction(5, 6), 1001, 5001, "b")
    assert [e for e in engine.advance(Fraction(3, 2)) if e.kind == "on"] == []
    assert [e.value for e in engine.advance(Fraction(7, 4)) if e.kind == "on"] == ["b"]
