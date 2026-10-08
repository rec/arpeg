from fractions import Fraction
from pathlib import Path

import mido
import pytest

from arpeg.history import LiveHistoryArpeggiator
from arpeg.midi import MidiPlayer
from arpeg.profile import parse_profile


def test_invalid_capture_controls_preserve_pending_output() -> None:
    player = MidiPlayer(
        profile=parse_profile(Path("conformance/phrase-wind.toml").read_text())
    )
    for command in ("commit", "overdub", "undo", "unknown"):
        snapshot = player.model_dump_json()
        with pytest.raises(ValueError):
            player.capture(100_000_000, command)
        assert player.model_dump_json() == snapshot
    player.capture(0, "record")
    player.accept(0, mido.Message.from_bytes([144, 60, 100]))
    snapshot = player.model_dump_json()
    with pytest.raises(ValueError, match="already recording"):
        player.capture(100_000_000, "record")
    assert player.model_dump_json() == snapshot
    player.capture(0, "commit")
    player.advance(0)
    with pytest.raises(ValueError, match="record before"):
        player.capture(100_000_000, "commit")
    assert [m.bytes() for m in player.advance(100_000_000)] == [[128, 60, 0]]


def test_take_limit_can_be_recovered_without_reusing_capture_identities() -> None:
    player = MidiPlayer(
        profile=parse_profile(Path("conformance/phrase-wind.toml").read_text())
    )
    for _ in range(128):
        player.capture(0, "record")
        player.capture(0, "overdub")
    player.capture(0, "record")
    assert isinstance(player.engine, LiveHistoryArpeggiator)
    assert player.engine.bank.recording is not None
    assert player.engine.bank.recording.capture_id == "take-128"
    with pytest.raises(ValueError, match="128-take limit"):
        player.capture(0, "commit")
    player.capture(0, "undo")
    player.capture(0, "commit")
    player.capture(0, "undo")
    player.capture(0, "record")
    assert player.engine.bank.recording is not None
    assert player.engine.bank.recording.capture_id == "take-129"
    player.clear(0)
    player.capture(0, "record")
    player.accept(0, mido.Message.from_bytes([144, 64, 90]))
    player.capture(0, "commit")
    assert [m.bytes() for m in player.advance(125_000_000)] == [[144, 64, 90]]
    assert player.engine.bank.published[0].capture_id == "take-130"
    assert player.clock.beat == Fraction(1, 4)
