from collections.abc import Callable
from contextlib import nullcontext
from io import StringIO
from pathlib import Path
from types import SimpleNamespace
from typing import Literal

import mido
import pytest

from arpeg import midi
from arpeg.inbox import Inbox, InputOverflow
from arpeg.output import OutputOverload


class Output:
    def __init__(self) -> None:
        self.messages: list[list[int]] = []

    def send(self, message: mido.Message) -> None:
        self.messages.append(message.bytes())


class Backend:
    def __init__(self) -> None:
        self.output = Output()
        self.receive: Callable[[mido.Message], None] | None = None

    def get_input_names(self) -> list[str]:
        return ["notes"]

    def get_output_names(self) -> list[str]:
        return ["synth"]

    def open_output(self, name: str) -> nullcontext[Output]:
        return nullcontext(self.output)

    def open_input(
        self, name: str, callback: Callable[[mido.Message], None]
    ) -> nullcontext[None]:
        self.receive = callback
        callback(mido.Message("note_on", note=60, velocity=100))
        return nullcontext()


@pytest.fixture
def host(monkeypatch: pytest.MonkeyPatch) -> Backend:
    backend = Backend()
    monkeypatch.setattr(midi.mido, "Backend", lambda _: backend)
    monkeypatch.setattr(midi.Thread, "start", lambda _: None)
    monkeypatch.setattr(midi, "perf_counter_ns", lambda: 0)
    return backend


@pytest.mark.parametrize(
    "limits, burst",
    [
        ({"input_packets": 1}, [mido.Message("clock"), mido.Message("clock")]),
        ({"input_bytes": 3}, [mido.Message("note_on"), mido.Message("clock")]),
        ({"max_message_bytes": 3}, [mido.Message("sysex", data=[1, 2])]),
    ],
)
def test_overflow_ends_play_and_releases_sounding_output(
    host: Backend,
    monkeypatch: pytest.MonkeyPatch,
    limits: dict[str, int],
    burst: list[mido.Message],
) -> None:
    def burst_input(_: float) -> None:
        assert host.receive is not None
        for m in burst:
            host.receive(m)

    monkeypatch.setattr(midi, "sleep", burst_input)
    with pytest.raises(InputOverflow, match="MIDI input overflow"):
        midi.Play(
            profile=Path("conformance/up.toml"), source=0, destination=0, **limits
        ).run()
    assert host.output.messages == [[0x90, 60, 100], [0x80, 60, 0]]


def test_overflow_preserves_a_release_already_produced_by_input(
    host: Backend,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    accept = midi.MidiPlayer.accept

    def accept_and_overflow(
        self: midi.MidiPlayer,
        at: int,
        message: mido.Message,
        *,
        source: Literal["notes", "clock", "both"] = "both",
    ) -> list[mido.Message]:
        messages = accept(self, at, message, source=source)
        if message.bytes()[0] & 0xF0 == 0x80:
            assert host.receive is not None
            host.receive(mido.Message("clock"))
            host.receive(mido.Message("clock"))
        return messages

    def release_input(_: float) -> None:
        assert host.receive is not None
        host.receive(mido.Message("note_off", note=60))

    monkeypatch.setattr(midi.MidiPlayer, "accept", accept_and_overflow)
    monkeypatch.setattr(midi, "sleep", release_input)
    with pytest.raises(InputOverflow):
        midi.Play(
            profile=Path("conformance/up.toml"),
            source=0,
            destination=0,
            input_packets=1,
        ).run()
    assert host.output.messages == [[0x90, 60, 100], [0x80, 60, 0]]


@pytest.mark.parametrize("text", ["pause\n" * 33, "x" * 1025])
def test_console_capacity_failure_ends_play(
    host: Backend,
    monkeypatch: pytest.MonkeyPatch,
    text: str,
) -> None:
    monkeypatch.setattr(midi, "stdin", StringIO(text))
    monkeypatch.setattr(midi.Thread, "start", lambda t: t.run())
    with pytest.raises(InputOverflow, match="Console input overflow"):
        midi.Play(profile=Path("conformance/up.toml"), source=0, destination=0).run()
    assert host.output.messages == []


def test_output_overload_releases_sent_note_instead_of_refused_attack(
    host: Backend,
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    now = 0

    def next_step(_: float) -> None:
        nonlocal now
        now = 125_000_000
        assert host.receive is not None
        host.receive(mido.Message("note_on", note=64, velocity=90))

    monkeypatch.setattr(midi, "perf_counter_ns", lambda: now)
    monkeypatch.setattr(midi, "sleep", next_step)
    with pytest.raises(OutputOverload):
        midi.Play(
            profile=Path("conformance/up.toml"),
            source=0,
            destination=0,
            output_bytes_per_second=1,
            output_burst_bytes=3,
        ).run()
    assert host.output.messages == [[0x90, 60, 100], [0x80, 60, 0]]


def test_register_error_during_a_tempo_command_ends_play_with_cleanup(
    host: Backend,
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    commands: Inbox[str] | None = None
    now = 0

    def command_thread(
        *, target: object, args: tuple[Inbox[str]], daemon: bool
    ) -> SimpleNamespace:
        nonlocal commands
        commands = args[0]
        return SimpleNamespace(start=lambda: None)

    def next_step(_: float) -> None:
        nonlocal now
        assert now == 0, "range error did not end the session"
        now = 125_000_000
        assert host.receive is not None and commands is not None
        host.receive(mido.Message("note_on", note=73, velocity=90))
        commands.put("tempo 90", 8)

    profile = tmp_path / "register.toml"
    profile.write_text("""title="Register"
[body]
rhythm={kind="grid",step="1/4 beat"}
gate=2
register_limits={low=60,high=72,boundary="error"}
""")
    monkeypatch.setattr(midi, "Thread", command_thread)
    monkeypatch.setattr(midi, "perf_counter_ns", lambda: now)
    monkeypatch.setattr(midi, "sleep", next_step)
    with pytest.raises(RuntimeError, match="register limits"):
        midi.Play(profile=profile, source=0, destination=0).run()
    assert host.output.messages == [[0x90, 60, 100], [0x80, 60, 0]]
