from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest
from meedee import codecs
from ufor.arpeggiator import ExpressionLane
from ufor.arpeggiator_ports import ArpeggiatorControl

from arpeg.expression import envelfo_message
from arpeg.history import LiveHistoryArpeggiator
from arpeg.midi import MidiPlayer, decode_message
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case", loads(Path("conformance/expression-values.toml").read_text())["cases"]
)
def test_normalized_samples_round_to_exact_destination_values(
    case: dict[str, object],
) -> None:
    assert (
        envelfo_message(ExpressionLane(str(case["port"])), Fraction(str(case["value"])))
        == case["data"]
    )


def test_unknown_envelfo_state_and_same_time_updates_survive_snapshot() -> None:
    player = MidiPlayer(
        profile=parse_profile(Path("conformance/envelfo-expression.toml").read_text())
    )
    player.accept(0, decode_message([144, 60, 100]))
    assert [list(codecs.encode(m)) for m in player.advance(0)] == [[144, 60, 100]]
    control = ArpeggiatorControl(port="breath", value=Fraction(1, 3))
    assert [list(codecs.encode(m)) for m in player.control(0, control)] == [
        [176, 2, 42]
    ]
    player = MidiPlayer.model_validate_json(player.model_dump_json())
    assert player.envelfo_expression == {ExpressionLane.breath: Fraction(1, 3)}
    assert [list(codecs.encode(m)) for m in player.control(0, control)] == [
        [176, 2, 42]
    ]
    assert [list(codecs.encode(m)) for m in player.advance(125_000_000)] == [
        [128, 60, 0],
        [176, 2, 42],
        [144, 60, 100],
    ]


def test_envelfo_requires_declared_ownership_before_changing_state() -> None:
    player = MidiPlayer(profile=parse_profile(Path("conformance/up.toml").read_text()))
    player.accept(0, decode_message([144, 60, 100]))
    before = player.model_dump_json()
    with pytest.raises(ValueError, match="not owned by Envelfo"):
        player.control(
            50_000_000, ArpeggiatorControl(port="breath", value=Fraction(1, 2))
        )
    assert player.model_dump_json() == before
    assert [list(codecs.encode(m)) for m in player.advance(0)] == [[144, 60, 100]]


def test_recorded_override_preserves_input_capture_with_global_current_source() -> None:
    text = (
        Path("conformance/envelfo-phrase.toml")
        .read_text()
        .replace('source = "recorded"', 'source = "current"')
        .replace('pressure = "current"', 'bend = "recorded"')
    )
    player = MidiPlayer(profile=parse_profile(text))
    player.accept(0, decode_message([224, 0, 64]))
    player.capture(0, "record")
    player.control(0, ArpeggiatorControl(port="breath", value=Fraction(1, 2)))
    player.accept(0, decode_message([144, 60, 100]))
    player.accept(10_000_000, decode_message([224, 0, 96]))
    player.accept(20_000_000, decode_message([128, 60, 0]))
    player.capture(20_000_000, "commit")
    assert [list(codecs.encode(m)) for m in player.advance(125_000_000)] == [
        [176, 2, 64],
        [224, 0, 64],
        [144, 60, 100],
    ]
    assert [
        list(codecs.encode(m))
        for m in player.control(125_000_000, ArpeggiatorControl(port="breath", value=1))
    ] == [[176, 2, 127]]
    assert [list(codecs.encode(m)) for m in player.advance(175_000_000)] == [
        [224, 0, 96]
    ]
    assert isinstance(player.engine, LiveHistoryArpeggiator)
    assert [e.data for e in player.engine.bank.source("take-0").events] == [
        [224, 0, 64],
        [144, 60, 100],
        [224, 0, 96],
        [128, 60, 0],
    ]
