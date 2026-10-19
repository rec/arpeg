from pathlib import Path

import pytest
import tyro
from meedee import codecs
from ufor.arpeggiator import ArpeggiatorScore
from ufor.codec import parse_score

from arpeg.midi import MidiPlayer, Play, decode_message


def test_classic_wire_messages_use_channel_one_and_callback_times() -> None:
    player = MidiPlayer(profile=_profile("up"))
    assert player.advance(5_000_000_000) == []
    assert player.accept(5_000_000_000, decode_message([0x91, 67, 100])) == []
    player.accept(10_000_000_000, decode_message([0x90, 60, 100]))
    player.accept(10_000_000_000, decode_message([0x90, 64, 90]))
    assert [list(codecs.encode(m)) for m in player.advance(10_125_000_000)] == [
        [0x90, 60, 100],
        [0x80, 60, 0],
        [0x90, 64, 90],
    ]
    player.accept(10_150_000_000, decode_message([0x90, 60, 0]))
    assert [
        list(codecs.encode(m))
        for m in player.accept(10_175_000_000, decode_message([0x80, 64, 20]))
    ] == [[0x80, 64, 0]]
    assert player.advance(10_250_000_000) == []


def test_history_wire_messages_preserve_recorded_breath_and_release_velocity() -> None:
    player = MidiPlayer(profile=_profile("history-wind"))
    player.accept(0, decode_message([0xB0, 2, 0]))
    player.accept(0, decode_message([0x90, 60, 100]))
    player.accept(15_625_000, decode_message([0xB0, 2, 80]))
    player.accept(62_500_000, decode_message([0x80, 60, 20]))
    assert [list(codecs.encode(m)) for m in player.advance(125_000_000)] == [
        [0xB0, 2, 0],
        [0x90, 60, 100],
    ]
    assert [list(codecs.encode(m)) for m in player.advance(150_000_000)] == [
        [0xB0, 2, 80]
    ]
    assert [list(codecs.encode(m)) for m in player.advance(225_000_000)] == [
        [0x80, 60, 20]
    ]


def test_late_history_input_moves_after_the_published_tick() -> None:
    player = MidiPlayer(profile=_profile("history-wind"))
    player.accept(0, decode_message([0x90, 60, 100]))
    player.advance(50_000_000)
    player.accept(40_000_000, decode_message([0x90, 60, 0]))
    assert [list(codecs.encode(m)) for m in player.advance(125_000_000)] == [
        [0x90, 60, 100]
    ]


@pytest.mark.parametrize("name", ["live-latch", "history-wind"])
def test_clear_and_stop_release_owned_output(name: str) -> None:
    player = MidiPlayer(profile=_profile(name))
    player.accept(0, decode_message([0x90, 60, 100]))
    player.accept(62_500_000, decode_message([0x80, 60, 0]))
    player.advance(125_000_000)
    assert [list(codecs.encode(m)) for m in player.clear(150_000_000)] == [
        [0x80, 60, 0]
    ]
    assert player.advance(250_000_000) == []
    assert player.stop(250_000_000) == []


def test_play_arguments_are_typed_pydantic_commands() -> None:
    command = tyro.cli(
        Play,
        args=[
            "--profile",
            "conformance/up.toml",
            "--source",
            "0",
            "--destination",
            "1",
        ],
    )
    assert command == Play(profile=Path("conformance/up.toml"), source=0, destination=1)


@pytest.mark.parametrize(
    "name",
    [
        "euclidean",
        "custom-steps",
        "weighted-walk",
        "alternating",
        "inside-out",
        "outside-in",
        "index-pattern",
        "shuffle",
        "choice",
        "live-wind",
        "history-live-wind",
    ],
)
def test_live_presets_prepare_without_opening_devices(name: str) -> None:
    player = MidiPlayer(profile=_profile(name))
    assert player.advance(0) == []


def _profile(name: str) -> ArpeggiatorScore:
    profile = parse_score(Path(f"conformance/{name}.toml").read_text())
    assert isinstance(profile, ArpeggiatorScore)
    return profile


@pytest.mark.parametrize("data", [b"", b"\x90\x3c", b"\xf8\xf8", b"\xf9"])
def test_single_message_conversion_rejects_missing_extra_or_malformed_data(
    data: bytes,
) -> None:
    with pytest.raises(ValueError, match="exactly one complete"):
        decode_message(data)


def test_cli_can_disable_default_isolation() -> None:
    command = tyro.cli(
        Play,
        args=[
            "--profile",
            "conformance/up.toml",
            "--source",
            "0",
            "--destination",
            "1",
            "--no-isolated",
        ],
    )
    assert not command.isolated
    assert Play(profile=Path("conformance/up.toml"), source=0, destination=1).isolated
