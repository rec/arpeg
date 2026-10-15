from fractions import Fraction
from pathlib import Path
from tomllib import loads

import mido
import pytest
from ufor import arpeggiator_ports
from ufor.arpeggiator import StepPatterns

from arpeg.engine import EngineSettings, TokenArpeggiator
from arpeg.live import LiveArpeggiator
from arpeg.midi import MidiPlayer
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/step-patterns.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
@pytest.mark.parametrize("polling", [False, True])
def test_step_patterns_match_shared_traces_and_snapshots(
    case: dict[str, object], polling: bool
) -> None:
    profile = parse_profile(Path("conformance/up.toml").read_text())
    profile = type(profile).model_validate(
        profile.model_dump()
        | {
            "body": profile.body.model_dump()
            | {"rhythm": case["rhythm"], "patterns": case["patterns"]}
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


def test_generic_events_preserve_exact_accents_without_changing_text_payload() -> None:
    engine = TokenArpeggiator(
        settings=EngineSettings(
            patterns=StepPatterns(accents=[Fraction(1, 2), Fraction(0), Fraction(2)])
        )
    )
    engine.press(Fraction(0), 1000, 5000, "λ")
    events = engine.advance(Fraction(1, 2))
    assert [(e.kind, e.value, e.accent) for e in events] == [
        ("on", "λ", Fraction(1, 2)),
        ("off", "λ", Fraction(1, 2)),
        ("on", "λ", Fraction(2)),
    ]


def test_rejected_chance_and_empty_banks_still_advance_pattern_phase() -> None:
    engine = TokenArpeggiator(
        settings=EngineSettings(
            seed=42,
            patterns=StepPatterns(accents=[Fraction(1), Fraction(2), Fraction(3)]),
        )
    )
    engine.advance(Fraction(0))
    engine.press(Fraction(1, 8), 1000, 5000, "a")
    engine.control(
        Fraction(1, 8), arpeggiator_ports.ArpeggiatorControl(port="density", value=0)
    )
    assert engine.advance(Fraction(1, 4)) == []
    engine.control(
        Fraction(1, 3), arpeggiator_ports.ArpeggiatorControl(port="density", value=1)
    )
    assert [(e.at, e.accent) for e in engine.advance(Fraction(1, 2))] == [
        (Fraction(1, 2), Fraction(3))
    ]


def test_pending_repeats_keep_accent_and_gate_after_a_bank_edit_and_control() -> None:
    case = loads(Path("conformance/step-patterns.toml").read_text())["cases"][-1]
    profile = parse_profile(Path("conformance/up.toml").read_text())
    profile = type(profile).model_validate(
        profile.model_dump()
        | {
            "body": profile.body.model_dump()
            | {"rhythm": case["rhythm"], "patterns": case["patterns"]}
        }
    )
    engine = LiveArpeggiator(profile=profile)
    engine.note_on(Fraction(0), 60, 100)
    engine.advance(Fraction(0))
    events = engine.note_on(Fraction(1, 8), 64, 80)
    engine.control(
        Fraction(1, 8), arpeggiator_ports.ArpeggiatorControl(port="gate", value=0)
    )
    engine = LiveArpeggiator.model_validate_json(engine.model_dump_json())
    events.extend(engine.advance(Fraction(7, 10)))
    assert [(e.kind, e.at, e.key, e.velocity) for e in events] == [
        ("off", Fraction(1, 10), 60, 0),
        ("on", Fraction(1, 4), 60, 50),
        ("off", Fraction(7, 10), 60, 0),
    ]


def test_history_accents_preserve_recorded_expression_and_release_velocity() -> None:
    profile = parse_profile(Path("conformance/history-wind.toml").read_text())
    profile = profile.model_copy(
        update={
            "body": profile.body.model_copy(
                update={
                    "patterns": StepPatterns(
                        accents=[Fraction(0), Fraction(1, 2)], gates=[Fraction(1, 2)]
                    )
                }
            )
        }
    )
    player = MidiPlayer(profile=profile)
    for at, data in [
        (0, [176, 2, 30]),
        (0, [144, 60, 81]),
        (50_000_000, [128, 60, 20]),
    ]:
        player.accept(at, mido.Message.from_bytes(data))
    assert [m.bytes() for m in player.advance(225_000_000)] == [
        [176, 2, 30],
        [144, 60, 41],
        [128, 60, 20],
    ]
