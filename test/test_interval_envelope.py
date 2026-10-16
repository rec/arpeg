from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest
from ufor import arpeggiator_ports
from ufor.arpeggiator import IntervalEnvelope, RankRange, StepPatterns

from arpeg.engine import EngineSettings, TokenArpeggiator
from arpeg.live import LiveArpeggiator
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/interval-envelopes.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
@pytest.mark.parametrize("polling", [False, True])
def test_interval_envelopes_match_shared_traces_and_snapshots(
    case: dict[str, object], polling: bool
) -> None:
    profile = parse_profile(Path("conformance/interval-envelope.toml").read_text())
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


@pytest.mark.parametrize("silence", ["chance", "accent"])
def test_silent_opportunities_advance_the_envelope(silence: str) -> None:
    engine = TokenArpeggiator(
        settings=EngineSettings(
            seed=42,
            selection=IntervalEnvelope(
                ranges=[
                    RankRange(min=1, max=1),
                    RankRange(min=1, max=1),
                    RankRange(min=-1, max=-1),
                ]
            ),
            patterns=StepPatterns(
                accents=[
                    Fraction(1),
                    Fraction(0 if silence == "accent" else 1),
                    Fraction(1),
                ]
            ),
        )
    )
    for identity, token in enumerate(["a", "b", "c"]):
        engine.press(Fraction(0), identity, identity, token)
    engine.advance(Fraction(0))
    if silence == "chance":
        engine.control(
            Fraction(1, 8),
            arpeggiator_ports.ArpeggiatorControl(port="density", value=0),
        )
    assert [e for e in engine.advance(Fraction(1, 4)) if e.kind == "on"] == []
    if silence == "chance":
        engine.control(
            Fraction(3, 8),
            arpeggiator_ports.ArpeggiatorControl(port="density", value=1),
        )
    assert [e.value for e in engine.advance(Fraction(1, 2)) if e.kind == "on"] == ["c"]


@pytest.mark.parametrize(
    "command,first,second", [("pause", "b", "a"), ("relocate", "a", "c")]
)
def test_transport_preserves_envelope_phase_with_existing_selector_resets(
    command: str, first: str, second: str
) -> None:
    engine = TokenArpeggiator(
        settings=EngineSettings(
            seed=42,
            selection=IntervalEnvelope(
                ranges=[
                    RankRange(min=1, max=1),
                    RankRange(min=-1, max=-1),
                    RankRange(min=0, max=0),
                ]
            ),
        )
    )
    for identity, token in enumerate(["a", "b", "c"]):
        engine.press(Fraction(0), identity, identity, token)
    engine.advance(Fraction(0))
    getattr(engine, command)(Fraction(5, 8))
    assert [e.value for e in engine.advance(Fraction(3, 4)) if e.kind == "on"] == [
        first
    ]
    assert [e.value for e in engine.advance(Fraction(1)) if e.kind == "on"] == [second]


def test_surviving_entries_follow_current_rank_and_removal_restarts_lowest() -> None:
    engine = TokenArpeggiator(
        settings=EngineSettings(
            seed=42,
            selection=IntervalEnvelope(ranges=[RankRange(min=1, max=1)]),
        )
    )
    for identity, token in enumerate(["a", "b", "c"]):
        engine.press(Fraction(0), identity, identity, token)
    assert [e.value for e in engine.advance(Fraction(1, 4)) if e.kind == "on"] == [
        "a",
        "b",
    ]
    engine.release(Fraction(3, 10), 0)
    assert [e.value for e in engine.advance(Fraction(1, 2)) if e.kind == "on"] == ["c"]
    engine.release(Fraction(3, 5), 2)
    engine.press(Fraction(2, 3), 3, -1, "d")
    assert [e.value for e in engine.advance(Fraction(3, 4)) if e.kind == "on"] == ["d"]
