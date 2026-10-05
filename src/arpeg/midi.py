"""Portable mido host for the Python reference engines."""

from fractions import Fraction
from functools import cached_property
from pathlib import Path
from queue import Empty, SimpleQueue
from sys import exit, stderr
from threading import Thread
from time import perf_counter_ns, sleep
from typing import Annotated, Self

import mido
import tyro
from pydantic import BaseModel, Field, model_validator
from ufor import arpeggiator
from ufor.codec import parse_score
from ufor.events import MidiEvent

from .bank import CaptureBank
from .history import LiveHistoryArpeggiator
from .live import LiveArpeggiator, LiveEvent


class ListPorts(BaseModel, frozen=True):
    """List MIDI source and destination indices."""

    def run(self) -> None:
        backend = mido.Backend("mido.backends.rtmidi")
        print("Sources:")
        for index, name in enumerate(backend.get_input_names()):
            print(f"  {index}: {name}")
        print("Destinations:")
        for index, name in enumerate(backend.get_output_names()):
            print(f"  {index}: {name}")


class Play(BaseModel, frozen=True):
    """Play a profile through MIDI ports on channel 1."""

    profile: Path
    source: int = Field(ge=0)
    destination: int = Field(ge=0)
    bpm: int = Field(default=120, ge=1, le=1000)

    def run(self) -> None:
        profile = parse_score(self.profile.read_text())
        if not isinstance(profile, arpeggiator.ArpeggiatorScore):
            raise ValueError("play requires an arpeggiator profile")
        player = MidiPlayer(profile=profile, bpm=self.bpm)
        backend = mido.Backend("mido.backends.rtmidi")
        sources = backend.get_input_names()
        destinations = backend.get_output_names()
        if self.source >= len(sources):
            raise ValueError("MIDI source index is unavailable")
        if self.destination >= len(destinations):
            raise ValueError("MIDI destination index is unavailable")
        incoming: SimpleQueue[tuple[int, mido.Message]] = SimpleQueue()
        commands: SimpleQueue[str] = SimpleQueue()

        def receive(message: mido.Message) -> None:
            incoming.put((perf_counter_ns(), message))

        Thread(target=_read_commands, args=(commands,), daemon=True).start()
        with (
            backend.open_output(destinations[self.destination]) as output,
            backend.open_input(sources[self.source], callback=receive),
        ):
            print(f"Playing at {self.bpm} BPM. Enter clear, quit, or an empty line.")
            stopped = False
            try:
                while not stopped:
                    messages: list[mido.Message] = []
                    while True:
                        try:
                            at, message = incoming.get_nowait()
                        except Empty:
                            break
                        messages.extend(player.accept(at, message))
                    while True:
                        try:
                            command = commands.get_nowait()
                        except Empty:
                            break
                        if command == "clear":
                            if isinstance(profile.body.bank, arpeggiator.HeldBank):
                                print(
                                    "clear requires a latched or history bank",
                                    file=stderr,
                                )
                            else:
                                messages.extend(player.clear(perf_counter_ns()))
                        else:
                            stopped = True
                    if not stopped:
                        messages.extend(player.advance(perf_counter_ns()))
                    for message in messages:
                        output.send(message)
                    sleep(0.001)
            except KeyboardInterrupt:
                pass
            finally:
                for message in player.stop(perf_counter_ns()):
                    output.send(message)


class MidiPlayer(BaseModel):
    """Convert timestamped mido messages into reference-engine output."""

    profile: arpeggiator.ArpeggiatorScore = Field(frozen=True)
    bpm: int = Field(default=120, ge=1, le=1000, frozen=True)
    origin_ns: int | None = None
    last: Fraction = Fraction(0)
    published_tick: int = -1
    input_tick: int = -1
    ordinal: int = 0

    @model_validator(mode="after")
    def supported_profile(self) -> Self:
        # Prepare and validate the engine before opening any MIDI devices.
        _ = self.engine
        return self

    @cached_property
    def engine(self) -> LiveArpeggiator | LiveHistoryArpeggiator:
        body = self.profile.body
        if not isinstance(body.bank, arpeggiator.HistoryBank):
            if (
                body.expression.source != "current"
                or body.expression.timing != "original"
            ):
                raise ValueError(
                    "held and latched playback require current, original expression"
                )
            return LiveArpeggiator(profile=self.profile)
        if not isinstance(body.rhythm, arpeggiator.Grid) or body.probability != 1:
            raise ValueError(
                "history playback requires grid rhythm without probability"
            )
        selection = body.selection
        if isinstance(selection, (arpeggiator.Ascending, arpeggiator.Descending)):
            if selection.key != "pitch" or (
                isinstance(selection, arpeggiator.Ascending) and selection.repeats != 1
            ):
                raise ValueError("history playback requires unrepeated pitch selection")
        elif not isinstance(selection, arpeggiator.Played):
            raise ValueError("history playback requires classic selection")
        if body.expression != arpeggiator.Expression(
            source="recorded", timing="fit", gaps="carry"
        ):
            raise ValueError(
                "history playback requires recorded, fit, carry expression"
            )
        return LiveHistoryArpeggiator(
            step=Fraction(body.rhythm.step.removesuffix(" beat"))
            * 60_000_000
            / self.bpm,
            gate=body.gate,
            bank=CaptureBank(
                mode="history",
                history_size=body.bank.notes,
                selection=selection.kind,
                direction=selection.direction
                if isinstance(selection, arpeggiator.Played)
                else "forward",
                retrigger_on_edit=body.retrigger == "bank_edit",
            ),
        )

    def accept(self, at_ns: int, message: mido.Message) -> list[mido.Message]:
        engine = self.engine
        data = message.bytes()
        if isinstance(engine, LiveArpeggiator) and (
            len(data) != 3 or data[0] not in (0x80, 0x90)
        ):
            return []
        if self.origin_ns is None:
            self.origin_ns = at_ns
        at = self._time(at_ns)
        if isinstance(engine, LiveArpeggiator):
            if data[0] == 0x90 and data[2] > 0:
                events = engine.note_on(at, data[1], data[2])
            else:
                events = engine.note_off(at, data[1])
            return _note_messages(events)
        tick = max(int(at), self.published_tick + 1, self.input_tick)
        events = engine.before(tick)
        if tick != self.input_tick:
            self.input_tick = tick
            self.ordinal = 0
        engine.accept(MidiEvent(tick=tick, ordinal=self.ordinal, data=data))
        self.ordinal += 1
        self.last = Fraction(tick)
        return [mido.Message.from_bytes(e.data) for e in events]

    def advance(self, at_ns: int) -> list[mido.Message]:
        if self.origin_ns is None:
            return []
        at = self._time(at_ns)
        if isinstance(engine := self.engine, LiveArpeggiator):
            return _note_messages(engine.advance(at))
        self.published_tick = int(at)
        return [mido.Message.from_bytes(e.data) for e in engine.advance(int(at))]

    def clear(self, at_ns: int) -> list[mido.Message]:
        at = self._time(at_ns)
        if isinstance(engine := self.engine, LiveArpeggiator):
            return _note_messages(engine.clear(at))
        return [mido.Message.from_bytes(e.data) for e in engine.clear(int(at))]

    def stop(self, at_ns: int) -> list[mido.Message]:
        at = self._time(at_ns)
        if isinstance(engine := self.engine, LiveArpeggiator):
            return _note_messages(engine.stop(at))
        return [mido.Message.from_bytes(e.data) for e in engine.stop(int(at))]

    def _time(self, at_ns: int) -> Fraction:
        elapsed = max(0, at_ns - self.origin_ns) if self.origin_ns is not None else 0
        at = Fraction(elapsed, 1000)
        if isinstance(self.engine, LiveArpeggiator):
            at = at * self.bpm / 60_000_000
        self.last = max(at, self.last)
        return self.last


def main() -> None:
    command = tyro.cli(
        Annotated[ListPorts, tyro.conf.subcommand(name="list-ports")]
        | Annotated[Play, tyro.conf.subcommand(name="play")]
    )
    try:
        command.run()
    except (ValueError, OSError, RuntimeError, ImportError) as error:
        exit(str(error))


def _note_messages(events: list[LiveEvent]) -> list[mido.Message]:
    return [
        mido.Message.from_bytes([0x90 if e.kind == "on" else 0x80, e.key, e.velocity])
        for e in events
    ]


def _read_commands(commands: SimpleQueue[str]) -> None:
    while True:
        try:
            command = input().strip()
        except EOFError:
            commands.put("quit")
            return
        if command in ("", "quit"):
            commands.put("quit")
            return
        if command == "clear":
            commands.put(command)
        else:
            print("enter clear, quit, or an empty line", file=stderr)
