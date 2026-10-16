from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest
from ufor.arpeggiator import HitStep, Pattern, RestStep, Transition

from arpeg.engine import EngineSettings, TokenArpeggiator
from arpeg.live import LiveArpeggiator
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/transitions.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
@pytest.mark.parametrize("polling", [False, True])
def test_transitions_match_shared_traces_and_snapshots(
    case: dict[str, object], polling: bool
) -> None:
    profile = parse_profile(Path("conformance/transition.toml").read_text())
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


def test_surviving_notes_use_their_current_rank_and_removal_restarts_the_opening() -> (
    None
):
    engine = TokenArpeggiator(
        settings=EngineSettings(
            seed=42,
            selection=Transition(
                weights=[[0, 1, 0], [0, 0, 1], [1, 0, 0]], missing_weight=0
            ),
        )
    )
    for identity, rank, token in [(1000, 0, "a"), (1001, 1, "b"), (1002, 2, "c")]:
        engine.press(Fraction(0), identity, rank, token)
    assert [e.value for e in engine.advance(Fraction(1, 4)) if e.kind == "on"] == [
        "a",
        "b",
    ]
    engine.release(Fraction(3, 10), 1000)
    assert [e.value for e in engine.advance(Fraction(1, 2)) if e.kind == "on"] == ["c"]
    engine.release(Fraction(3, 5), 1002)
    engine.press(Fraction(2, 3), 1003, -1, "d")
    assert [e.value for e in engine.advance(Fraction(3, 4)) if e.kind == "on"] == ["d"]


def test_blocked_rests_preserve_the_next_transition_draw() -> None:
    traces = []
    for skip in [False, True]:
        engine = TokenArpeggiator(
            settings=EngineSettings(
                seed=42,
                selection=Transition(
                    weights=[[0, 3, 1], [1, 0, 1], [4, 1, 0]], missing_weight=0
                ),
            )
        )
        engine.press(Fraction(0), 1000, 0, "a")
        engine.advance(Fraction(0))
        if skip:
            engine.pause(Fraction(3, 5))
        else:
            assert [e for e in engine.advance(Fraction(3, 5)) if e.kind == "on"] == []
        engine.press(Fraction(3, 5), 1001, 1, "b")
        engine.press(Fraction(3, 5), 1002, 2, "c")
        traces.append(
            [(e.at, e.value) for e in engine.advance(Fraction(2)) if e.kind == "on"]
        )
    assert traces[0] == traces[1]
    assert traces[0]


def test_repeats_and_rhythm_rests_do_not_advance_transitions() -> None:
    engine = TokenArpeggiator(
        settings=EngineSettings(
            seed=42,
            selection=Transition(weights=[[0, 1], [1, 0]], missing_weight=0),
            rhythm=Pattern(
                steps=[
                    HitStep(duration="1/2 beat", repeats=2),
                    RestStep(duration="1/2 beat"),
                ]
            ),
        )
    )
    engine.press(Fraction(0), 1000, 0, "a")
    engine.press(Fraction(0), 1001, 1, "b")
    assert [
        (e.at, e.value) for e in engine.advance(Fraction(5, 4)) if e.kind == "on"
    ] == [
        (Fraction(0), "a"),
        (Fraction(1, 4), "a"),
        (Fraction(1), "b"),
        (Fraction(5, 4), "b"),
    ]
