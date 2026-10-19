from __future__ import annotations

from collections.abc import Callable
from contextlib import nullcontext
from io import StringIO
from pathlib import Path
from types import SimpleNamespace
from typing import Literal

import pytest
from meedee import codecs, events
from meedee.messages import (
    Message,
    NoteOff,
    NoteOn,
    SysEx,
    TimingClock,
)

from arpeg import midi
from arpeg.inbox import Inbox, InputOverflow
from arpeg.output import OutputOverload


class Output:
    worker_failure: Exception | None = None

    def __init__(self) -> None:
        self.messages: list[list[int]] = []

    def ports(self) -> list[SimpleNamespace]:
        return [SimpleNamespace(name="synth")]

    def connect(self, port: SimpleNamespace) -> nullcontext[Output]:
        return nullcontext(self)

    def send(self, packet: bytes) -> None:
        self.messages.append(list(packet))


class Input:
    dropped_packets: int = 0
    worker_failure: Exception | None = None
    closed: bool = False

    def __init__(self, host: Backend) -> None:
        self.host = host

    def ports(self) -> list[SimpleNamespace]:
        return [SimpleNamespace(name="notes")]

    def connect(
        self, port: SimpleNamespace, callback: Callable[[events.ReceivedPacket], None]
    ) -> nullcontext[Input]:
        self.host.receive = callback
        self.host.emit(NoteOn(channel=1, note=60, velocity=100))
        return nullcontext(self)


class Backend:
    def __init__(self) -> None:
        self.output = Output()
        self.input = Input(self)
        self.receive: Callable[[events.ReceivedPacket], None] | None = None
        self.sequence = 0

    def packet(self, data: bytes, *, sequence: int | None = None) -> None:
        assert self.receive is not None
        self.receive(
            events.ReceivedPacket(
                timestamp_ns=-123,
                clock=events.Clock(
                    identity="test",
                    backend=midi.midi.Backend.CoreMidi,
                    mode=midi.midi.TimestampMode.Absolute,
                ),
                packet=data,
                sequence=self.sequence if sequence is None else sequence,
            )
        )
        self.sequence += 1

    def emit(self, message: Message) -> None:
        self.packet(codecs.encode(message))


@pytest.fixture
def host(monkeypatch: pytest.MonkeyPatch) -> Backend:
    backend = Backend()
    monkeypatch.setattr(midi.midi, "Input", lambda **_: backend.input)
    monkeypatch.setattr(midi.midi, "Output", lambda **_: backend.output)
    monkeypatch.setattr(midi.Thread, "start", lambda _: None)
    monkeypatch.setattr(midi, "perf_counter_ns", lambda: 0)
    return backend


@pytest.mark.parametrize(
    "limits, burst",
    [
        ({"input_packets": 1}, [TimingClock(), TimingClock()]),
        ({"input_bytes": 3}, [NoteOn(channel=1, note=0, velocity=64), TimingClock()]),
        ({"max_message_bytes": 3}, [SysEx(data=bytes([1, 2]))]),
    ],
)
def test_overflow_ends_play_and_releases_sounding_output(
    host: Backend,
    monkeypatch: pytest.MonkeyPatch,
    limits: dict[str, int],
    burst: list[Message],
) -> None:
    def burst_input(_: float) -> None:
        assert host.receive is not None
        for m in burst:
            host.emit(m)

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
        message: Message,
        *,
        source: Literal["notes", "clock", "both"] = "both",
    ) -> list[Message]:
        messages = accept(self, at, message, source=source)
        if list(codecs.encode(message))[0] & 0xF0 == 0x80:
            assert host.receive is not None
            host.emit(TimingClock())
            host.emit(TimingClock())
        return messages

    def release_input(_: float) -> None:
        assert host.receive is not None
        host.emit(NoteOff(channel=1, note=60, velocity=64))

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
        host.emit(NoteOn(channel=1, note=64, velocity=90))

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
        host.emit(NoteOn(channel=1, note=73, velocity=90))
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


@pytest.mark.parametrize(
    "data, diagnostic",
    [
        (b"\xf6\x01", "UnexpectedData"),
        (b"\xf9", "UndefinedStatus"),
        (b"\xf0\x01\x90\x3c\x64", "InterruptedSysEx"),
    ],
)
def test_decoder_diagnostics_end_play_and_release_sent_notes(
    host: Backend, monkeypatch: pytest.MonkeyPatch, data: bytes, diagnostic: str
) -> None:
    monkeypatch.setattr(midi, "sleep", lambda _: host.packet(data))
    with pytest.raises(InputOverflow, match=diagnostic):
        midi.Play(profile=Path("conformance/up.toml"), source=0, destination=0).run()
    assert host.output.messages == [[0x90, 60, 100], [0x80, 60, 0]]


def test_fragmented_input_uses_completing_packet_arrival_and_preserves_release(
    host: Backend, monkeypatch: pytest.MonkeyPatch
) -> None:
    received: list[tuple[int, Message]] = []
    accept = midi.MidiPlayer.accept
    now = 0

    def observe(
        self: midi.MidiPlayer,
        at: int,
        message: Message,
        *,
        source: Literal["notes", "clock", "both"] = "both",
    ) -> list[Message]:
        received.append((at, message))
        return accept(self, at, message, source=source)

    def feed(_: float) -> None:
        nonlocal now
        now += 20_000_000
        if now == 20_000_000:
            host.packet(b"\x80\x3c")
        elif now == 40_000_000:
            assert host.output.messages == [[0x90, 60, 100]]
            host.packet(b"\x14")
        else:
            host.packet(b"\xf9")

    monkeypatch.setattr(midi, "perf_counter_ns", lambda: now)
    monkeypatch.setattr(midi.MidiPlayer, "accept", observe)
    monkeypatch.setattr(midi, "sleep", feed)
    with pytest.raises(InputOverflow, match="UndefinedStatus"):
        midi.Play(profile=Path("conformance/up.toml"), source=0, destination=0).run()
    assert received == [
        (0, NoteOn(channel=1, note=60, velocity=100)),
        (40_000_000, NoteOff(channel=1, note=60, velocity=20)),
    ]
    assert host.output.messages == [[0x90, 60, 100], [0x80, 60, 0]]


@pytest.mark.parametrize("failure", ["sequence", "drops", "worker"])
def test_transport_loss_stops_before_processing_more_input(
    host: Backend, monkeypatch: pytest.MonkeyPatch, failure: str
) -> None:
    def lose(_: float) -> None:
        if failure == "sequence":
            host.packet(b"\xf8", sequence=3)
        elif failure == "drops":
            host.input.dropped_packets = 1
        else:
            host.input.worker_failure = midi.midi.WorkerError("test worker failed")

    monkeypatch.setattr(midi, "sleep", lose)
    with pytest.raises(RuntimeError, match="input lost|worker failed"):
        midi.Play(profile=Path("conformance/up.toml"), source=0, destination=0).run()
    assert host.output.messages == [[0x90, 60, 100], [0x80, 60, 0]]


def test_fragmented_sysex_cannot_exceed_assembled_limit(
    host: Backend, monkeypatch: pytest.MonkeyPatch
) -> None:
    fragments = iter([b"\xf0\x01", b"\x02\xf7"])
    monkeypatch.setattr(midi, "sleep", lambda _: host.packet(next(fragments)))
    with pytest.raises(InputOverflow, match="SysExTooLarge"):
        midi.Play(
            profile=Path("conformance/up.toml"),
            source=0,
            destination=0,
            max_message_bytes=3,
        ).run()
    assert host.output.messages == [[0x90, 60, 100], [0x80, 60, 0]]
