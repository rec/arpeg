"""Portable mido host for the Python reference engines."""

from contextlib import ExitStack
from fractions import Fraction
from pathlib import Path
from queue import Empty
from sys import exit, stderr, stdin
from threading import Thread
from time import perf_counter_ns, sleep
from typing import Annotated, Literal, Self

import mido
import tyro
from pydantic import BaseModel, Field, model_validator
from ufor import arpeggiator, arpeggiator_ports
from ufor.events import MidiEvent

from .bank import CaptureBank
from .clock import ClockMode, TransportClock
from .expression import entry_messages, envelfo_message, expression_lane, tuning_message
from .gesture import RealizedMidiEvent
from .history import LiveHistoryArpeggiator
from .inbox import Inbox, InputFailure
from .live import LiveArpeggiator, LiveEvent
from .output import MidiDelivery
from .ports import PerformancePorts
from .profile import parse_profile
from .tuning import PreparedTuning


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
    clock: ClockMode = ClockMode.internal
    clock_source: int | None = Field(default=None, ge=0)
    clock_timeout_ms: int = Field(default=500, gt=0)
    input_packets: int = Field(default=4096, gt=0)
    input_bytes: int = Field(default=1048576, gt=0)
    max_message_bytes: int = Field(default=65536, gt=0)
    output_bytes_per_second: int | None = Field(default=None, gt=0)
    output_burst_bytes: int = Field(default=64, gt=0)

    def run(self) -> None:
        profile = parse_profile(self.profile.read_text(), self.profile)
        if self.clock_source is not None and self.clock != ClockMode.external:
            raise ValueError("clock source requires external clock")
        player = MidiPlayer(
            profile=profile,
            clock=TransportClock(
                mode=self.clock, bpm=self.bpm, timeout_us=self.clock_timeout_ms * 1000
            ),
        )
        backend = mido.Backend("mido.backends.rtmidi")
        sources = backend.get_input_names()
        destinations = backend.get_output_names()
        if self.source >= len(sources):
            raise ValueError("MIDI source index is unavailable")
        if self.destination >= len(destinations):
            raise ValueError("MIDI destination index is unavailable")
        if self.clock_source is not None and self.clock_source >= len(sources):
            raise ValueError("MIDI clock source index is unavailable")
        separate = self.clock_source is not None and self.clock_source != self.source
        failure = InputFailure()
        incoming = Inbox[tuple[int, mido.Message, Literal["notes", "clock", "both"]]](
            packets=self.input_packets,
            bytes=self.input_bytes,
            max_message_bytes=self.max_message_bytes,
            failure=failure,
        )
        commands = Inbox[str](
            packets=32,
            bytes=32768,
            max_message_bytes=1024,
            failure=failure,
            label="Console input",
        )
        delivery = MidiDelivery(
            bytes_per_second=self.output_bytes_per_second,
            burst_bytes=self.output_burst_bytes,
        )

        def receive(message: mido.Message) -> None:
            incoming.put(
                (perf_counter_ns(), message, "notes" if separate else "both"),
                len(message),
            )

        def receive_clock(message: mido.Message) -> None:
            incoming.put((perf_counter_ns(), message, "clock"), len(message))

        Thread(target=_read_commands, args=(commands,), daemon=True).start()
        with ExitStack() as stack:
            output = stack.enter_context(
                backend.open_output(destinations[self.destination])
            )
            stack.enter_context(
                backend.open_input(sources[self.source], callback=receive)
            )
            if separate:
                assert self.clock_source is not None
                stack.enter_context(
                    backend.open_input(
                        sources[self.clock_source], callback=receive_clock
                    )
                )
            print(
                f"Playing with {self.clock} clock. "
                "Enter start, pause, continue, tempo BPM, gate FRACTION, "
                "density FRACTION, transposition SEMITONES, selection_offset RANKS, "
                "breath FRACTION, bend FRACTION, pressure FRACTION, "
                "record, commit, "
                "overdub, undo, clear, or quit."
            )
            stopped = False
            try:
                while not stopped:
                    for _ in range(self.input_packets):
                        try:
                            at, message, source = incoming.get_nowait()
                        except Empty:
                            break
                        delivery.send(
                            player.accept(at, message, source=source),
                            perf_counter_ns(),
                            output.send,
                        )
                    for _ in range(commands.packets):
                        try:
                            command = commands.get_nowait()
                        except Empty:
                            break
                        messages: list[mido.Message] = []
                        if command == "clear":
                            if isinstance(profile.body.bank, arpeggiator.HeldBank):
                                print(
                                    "clear requires a latched, history or phrase bank",
                                    file=stderr,
                                )
                            else:
                                messages.extend(player.clear(perf_counter_ns()))
                        elif command in ("start", "pause", "continue"):
                            status = {"start": 0xFA, "pause": 0xFC, "continue": 0xFB}[
                                command
                            ]
                            messages.extend(
                                player.accept(
                                    perf_counter_ns(), mido.Message.from_bytes([status])
                                )
                            )
                        elif command in ("record", "commit", "overdub", "undo"):
                            try:
                                messages.extend(
                                    player.capture(perf_counter_ns(), command)
                                )
                                assert isinstance(player.engine, LiveHistoryArpeggiator)
                                bank = player.engine.bank
                                recording = bank.recording is not None
                                print(
                                    f"{command}: recording={recording}, "
                                    f"notes={len(bank.published)}, "
                                    f"revision={bank.revision}. "
                                    "Committed edits publish at the next step."
                                )
                            except ValueError as error:
                                print(str(error), file=stderr)
                        elif command.startswith("tempo "):
                            try:
                                messages.extend(
                                    player.set_tempo(
                                        perf_counter_ns(), int(command.split()[1])
                                    )
                                )
                            except ValueError as error:
                                print(str(error), file=stderr)
                        elif command.startswith(
                            (
                                "gate ",
                                "density ",
                                "transposition ",
                                "selection_offset ",
                                "breath ",
                                "bend ",
                                "pressure ",
                            )
                        ):
                            try:
                                port, value = command.split()
                                messages.extend(
                                    player.control(
                                        perf_counter_ns(),
                                        arpeggiator_ports.ArpeggiatorControl.model_validate(
                                            {"port": port, "value": value}
                                        ),
                                    )
                                )
                            except ValueError as error:
                                print(str(error), file=stderr)
                        else:
                            stopped = True
                        delivery.send(messages, perf_counter_ns(), output.send)
                    failure.check()
                    if not stopped:
                        delivery.send(
                            player.advance(perf_counter_ns()),
                            perf_counter_ns(),
                            output.send,
                        )
                    if player.take_events().exhausted:
                        print(
                            "Arpeggiator event buffers exhausted; "
                            "skipped attacks or steps",
                            file=stderr,
                        )
                    sleep(0.001)
            except KeyboardInterrupt:
                pass
            finally:
                delivery.cleanup(player.stop(perf_counter_ns()), output.send)


class ScheduledMidi(BaseModel, frozen=True):
    """Engine output awaiting expression and MIDI realization at a host boundary."""

    active: bool
    notes: list[LiveEvent] = Field(default_factory=list)
    history: list[RealizedMidiEvent] = Field(default_factory=list)
    release: bool = False


class MidiPlayer(BaseModel):
    """Convert source microseconds and transport beats into owned MIDI output."""

    profile: arpeggiator.ArpeggiatorScore = Field(frozen=True)
    clock: TransportClock = Field(default_factory=TransportClock)
    origin_ns: int | None = None
    wall_us: int = 0
    input_tick: int = -1
    ordinal: int = 0
    scheduled: ScheduledMidi | None = None
    engine: LiveArpeggiator | LiveHistoryArpeggiator | None = None
    expression: dict[int, list[int]] = Field(default_factory=dict)
    envelfo_expression: dict[arpeggiator.ExpressionLane, Fraction] = Field(
        default_factory=dict
    )
    output_id: int | None = None
    output_key: int | None = None
    tuning_offset: float = 0
    next_occurrence: int = 0
    output_note: arpeggiator_ports.ArpeggiatorNoteEvent | None = None
    notes: list[arpeggiator_ports.ArpeggiatorNoteEvent] = Field(
        default_factory=list, max_length=4096
    )
    notes_exhausted: bool = False
    suppressed_note: bool = False

    @model_validator(mode="after")
    def supported_profile(self) -> Self:
        if self.engine is None:
            self.engine = self._prepare_engine()
        return self

    def _prepare_engine(self) -> LiveArpeggiator | LiveHistoryArpeggiator:
        body = self.profile.body
        if not isinstance(body.bank, (arpeggiator.HistoryBank, arpeggiator.PhraseBank)):
            if (
                body.expression.source != "current"
                or body.expression.timing != "original"
                or "recorded" in body.expression.lanes.values()
            ):
                raise ValueError(
                    "held and latched playback require current, original expression"
                )
            return LiveArpeggiator(profile=self.profile)
        if not isinstance(body.rhythm, arpeggiator.Grid) or body.probability != 1:
            raise ValueError(
                "captured playback requires grid rhythm without probability"
            )
        selection = body.selection
        if isinstance(selection, (arpeggiator.Ascending, arpeggiator.Descending)):
            if selection.key != "pitch" or (
                isinstance(selection, arpeggiator.Ascending) and selection.repeats != 1
            ):
                raise ValueError(
                    "captured playback requires unrepeated pitch selection"
                )
        elif not isinstance(selection, arpeggiator.Played):
            raise ValueError("captured playback requires classic selection")
        if body.expression.source not in ("recorded", "current") or (
            body.expression.timing != "fit" or body.expression.gaps != "carry"
        ):
            raise ValueError(
                "captured playback requires recorded or current, fit, carry expression"
            )
        return LiveHistoryArpeggiator(
            ports=PerformancePorts(
                gate=body.gate,
                density=body.probability,
                transposition=body.transposition,
                selection_offset=body.selection_offset,
                register_limits=body.register_limits,
                scale=body.scale,
                tuning=PreparedTuning.prepare(body),
            ),
            seed=body.seed,
            name=self.profile.name,
            step=Fraction(body.rhythm.step.removesuffix(" beat")),
            swing=body.rhythm.swing,
            gate=body.gate,
            expression_source="recorded",
            bank=CaptureBank(
                octaves=body.octaves,
                mode=body.bank.kind,
                history_size=body.bank.notes
                if isinstance(body.bank, arpeggiator.HistoryBank)
                else 8,
                selection=selection.kind,
                direction=selection.direction
                if isinstance(selection, arpeggiator.Played)
                else "forward",
                retrigger_on_edit=body.retrigger == "bank_edit",
            ),
        )

    def accept(
        self,
        at_ns: int,
        message: mido.Message,
        *,
        source: Literal["notes", "clock", "both"] = "both",
    ) -> list[mido.Message]:
        self._check_ready()
        assert self.engine is not None
        data = message.bytes()
        transport = data[0] in (0xF8, 0xFA, 0xFB, 0xFC, 0xF2)
        if transport:
            if (
                source == "notes"
                or data[0] == 0xF8
                and self.clock.mode == ClockMode.internal
            ):
                return []
            tick = self._elapsed(at_ns)
            relocated = self.clock.accept(tick, data)
            if relocated:
                if isinstance(engine := self.engine, LiveArpeggiator):
                    return self._note_messages(engine.relocate(self.clock.beat))
                return self._history_messages(engine.relocate(self.clock.beat, tick))
            if not self.clock.active:
                return self._pause(tick)
            return []
        if source == "clock":
            return []
        engine = self.engine
        expressive = data[0] in (0xD0, 0xE0) or data[0] == 0xB0 and data[1] == 2
        if (
            isinstance(engine, LiveArpeggiator)
            and not expressive
            and (len(data) != 3 or data[0] not in (0x80, 0x90))
        ):
            return []
        tick = self._elapsed(at_ns)
        was_active = self.clock.active
        at = self.clock.advance(tick)
        output = self._pause(tick) if was_active and not self.clock.active else []
        if isinstance(engine, LiveArpeggiator):
            if expressive:
                output.extend(self._note_messages(engine.before(at)))
                self.expression[data[0]] = data
                lane = expression_lane(data)
                assert lane is not None
                policy = self.profile.body.expression
                if (
                    policy.lanes.get(lane, policy.source) == "current"
                    and self.clock.active
                    and self.output_key is not None
                ):
                    output.append(
                        mido.Message.from_bytes(
                            tuning_message(
                                data, self.tuning_offset, self.profile.body.bend_range
                            )
                        )
                    )
                return output
            events = (
                engine.note_on(at, data[1], data[2])
                if data[0] == 0x90 and data[2] > 0
                else engine.note_off(at, data[1])
            )
            return output + self._note_messages(events)
        tick = max(tick, engine.capture_to + int(engine.inclusive), self.input_tick)
        events = (
            engine.before(at, tick)
            if self.clock.active and at > engine.processed_to
            else []
        )
        if tick != self.input_tick:
            self.input_tick = tick
            self.ordinal = 0
        engine.accept(MidiEvent(tick=tick, ordinal=self.ordinal, data=data))
        self.ordinal += 1
        output.extend(self._history_messages(events))
        if expressive:
            self.expression[data[0]] = data
            lane = expression_lane(data)
            assert lane is not None
            if (
                self.profile.body.expression.lanes.get(
                    lane, self.profile.body.expression.source
                )
                == "current"
                and self.clock.active
                and self.output_key is not None
            ):
                output.append(
                    mido.Message.from_bytes(
                        tuning_message(
                            data, self.tuning_offset, self.profile.body.bend_range
                        )
                    )
                )
        return output

    def advance(self, at_ns: int) -> list[mido.Message]:
        self.schedule(at_ns)
        return self.realize()

    def schedule(self, at_ns: int, *, tempo: int | None = None) -> None:
        """Advance once without MIDI realization; drain hits before ``realize``.

        Hosts supplying expression must visit each scheduled boundary in order.
        A tempo change applies after settling this boundary, as in ``set_tempo``.
        """
        self._check_ready()
        assert self.engine is not None
        if tempo is not None and (
            self.clock.mode != ClockMode.internal or not 1 <= tempo <= 1000
        ):
            raise ValueError("tempo requires internal clock and BPM between 1 and 1000")
        notes: list[LiveEvent] = []
        history: list[RealizedMidiEvent] = []
        release = False
        if self.origin_ns is not None:
            tick = self._elapsed(at_ns)
            was_active = self.clock.active
            at = self.clock.advance(tick)
            release = was_active and not self.clock.active
            if release:
                if isinstance(engine := self.engine, LiveArpeggiator):
                    notes = engine.pause(at)
                else:
                    history = engine.pause(at, tick)
            elif self.clock.active:
                if isinstance(engine := self.engine, LiveArpeggiator):
                    notes = engine.advance(at)
                else:
                    history = engine.advance(at, max(tick, self.input_tick))
        if tempo is not None:
            self.clock.set_tempo(self._elapsed(at_ns), tempo)
        self.scheduled = ScheduledMidi(
            active=self.clock.active,
            notes=notes,
            history=history,
            release=release,
        )

    def realize(
        self, controls: list[arpeggiator_ports.ArpeggiatorControl] | None = None
    ) -> list[mido.Message]:
        """Apply expression to pending attacks, or to a continuing note once.

        Invalid control ownership leaves the pending batch available for correction.
        No clock advancement, new hit, or extra observation is implicit.
        """
        if (scheduled := self.scheduled) is None:
            raise ValueError("schedule MIDI before realization")
        samples = [self._expression_sample(c) for c in controls or []]
        attacks = [e.at for e in scheduled.notes if e.kind == "on"] + [
            e.at for e in scheduled.history if e.data[0] == 0x90 and e.data[2] > 0
        ]
        if samples and attacks and any(t != attacks[0] for t in attacks):
            raise ValueError("expression requires scheduling each attack boundary")
        for lane, value, _ in samples:
            self.envelfo_expression[lane] = value
        output = self._note_messages(scheduled.notes) + self._history_messages(
            scheduled.history
        )
        if scheduled.release:
            output.extend(self._release_output())
        if samples and not attacks and scheduled.active and self.output_key is not None:
            output.extend(
                mido.Message.from_bytes(
                    tuning_message(d, self.tuning_offset, self.profile.body.bend_range)
                )
                for _, _, d in samples
            )
        self.scheduled = None
        return output

    def clear(self, at_ns: int) -> list[mido.Message]:
        assert self.engine is not None
        output = self.advance(at_ns)
        tick = self._elapsed(at_ns)
        at = self.clock.advance(tick)
        if isinstance(engine := self.engine, LiveArpeggiator):
            return output + self._note_messages(engine.clear(at))
        return output + self._history_messages(engine.clear(at, tick))

    def stop(self, at_ns: int) -> list[mido.Message]:
        tick = self._elapsed(at_ns)
        self.clock.halt(tick)
        return self._pause(tick)

    def set_tempo(self, at_ns: int, bpm: int) -> list[mido.Message]:
        self.schedule(at_ns, tempo=bpm)
        return self.realize()

    def capture(self, at_ns: int, command: str) -> list[mido.Message]:
        self._check_ready()
        if (
            not isinstance(self.engine, LiveHistoryArpeggiator)
            or self.engine.bank.mode != "phrase"
        ):
            raise ValueError("capture controls require a phrase bank")
        self.engine.bank.check_control(command)
        if (
            command == "record"
            and self.engine.input_events + len(self.expression) > 1_000_000
        ):
            raise ValueError("live capture reached its event limit")
        tick = max(
            self._elapsed(at_ns),
            self.input_tick + 1,
            self.engine.capture_to + int(self.engine.inclusive),
        )
        was_active = self.clock.active
        at = self.clock.advance(self.wall_us)
        output = self._pause(tick) if was_active and not self.clock.active else []
        output.extend(self._history_messages(self.engine.capture(command, at, tick)))
        self.input_tick = tick
        self.ordinal = 0
        if command == "record":
            for status in (0xB0, 0xE0, 0xD0):
                if status in self.expression:
                    self.engine.accept(
                        MidiEvent(
                            tick=tick,
                            ordinal=self.ordinal,
                            data=self.expression[status],
                        )
                    )
                    self.ordinal += 1
        return output

    def control(
        self, at_ns: int, control: arpeggiator_ports.ArpeggiatorControl
    ) -> list[mido.Message]:
        self._check_ready()
        assert self.engine is not None and self.engine.ports is not None
        lane = (
            arpeggiator.ExpressionLane(control.port.value)
            if control.port.value in ("breath", "bend", "pressure")
            else None
        )
        if lane is not None:
            _, _, data = self._expression_sample(control)
        else:
            self.engine.ports.check_control(control, self.profile.body.seed)
        tick = self._elapsed(at_ns)
        was_active = self.clock.active
        at = self.clock.advance(tick)
        output = self._pause(tick) if was_active and not self.clock.active else []
        if lane is not None:
            if self.clock.active:
                if isinstance(self.engine, LiveArpeggiator):
                    output.extend(self._note_messages(self.engine.before(at)))
                elif at > self.engine.processed_to:
                    output.extend(
                        self._history_messages(
                            self.engine.before(at, max(tick, self.input_tick))
                        )
                    )
            self.envelfo_expression[lane] = control.value
            if self.clock.active and self.output_key is not None:
                output.append(
                    mido.Message.from_bytes(
                        tuning_message(
                            data,
                            self.tuning_offset,
                            self.profile.body.bend_range,
                        )
                    )
                )
            return output
        if isinstance(self.engine, LiveArpeggiator):
            output.extend(self._note_messages(self.engine.control(at, control)))
        else:
            output.extend(
                self._history_messages(
                    self.engine.control(at, max(tick, self.input_tick), control)
                )
            )
        return output

    def take_events(self) -> arpeggiator_ports.ArpeggiatorPortBatch:
        assert self.engine is not None and self.engine.ports is not None
        batch = self.engine.ports.take_events()
        batch = batch.model_copy(
            update={
                "notes": self.notes,
                "exhausted": batch.exhausted or self.notes_exhausted,
            }
        )
        self.notes = []
        self.notes_exhausted = False
        return batch

    def _start_note(self, at: Fraction, source: str, key: int, velocity: int) -> bool:
        if len(self.notes) > 4094:
            self.notes_exhausted = True
            self.suppressed_note = True
            return False
        note = arpeggiator_ports.ArpeggiatorNoteEvent(
            at=at,
            port="note_start",
            occurrence=self.next_occurrence,
            source=source,
            key=key,
            velocity=velocity,
        )
        self.next_occurrence += 1
        self.output_note = note
        self.notes.append(note)
        self.suppressed_note = False
        return True

    def _end_note(self, at: Fraction, velocity: int) -> None:
        if self.output_note is not None:
            self.notes.append(
                self.output_note.model_copy(
                    update={
                        "at": at,
                        "port": arpeggiator_ports.ArpeggiatorNotePort.note_end,
                        "velocity": velocity,
                    }
                )
            )
            self.output_note = None

    def _pause(self, tick: int) -> list[mido.Message]:
        assert self.engine is not None
        cancelled = self.scheduled is not None
        self.scheduled = None
        if isinstance(engine := self.engine, LiveArpeggiator):
            notes = engine.pause(self.clock.beat)
            output = [] if cancelled else self._note_messages(notes)
        else:
            history = engine.pause(self.clock.beat, tick)
            output = [] if cancelled else self._history_messages(history)
        return output + self._release_output()

    def _release_output(self) -> list[mido.Message]:
        output: list[mido.Message] = []
        if self.output_key is not None:
            output.append(mido.Message.from_bytes([128, self.output_key, 0]))
            self._end_note(self.clock.beat, 0)
            self.output_key = None
            self.output_id = None
        return output

    def _check_ready(self) -> None:
        if self.scheduled is not None:
            raise ValueError("realize scheduled MIDI before changing the player")

    def _expression_sample(
        self, control: arpeggiator_ports.ArpeggiatorControl
    ) -> tuple[arpeggiator.ExpressionLane, Fraction, list[int]]:
        if control.port.value not in ("breath", "bend", "pressure"):
            raise ValueError("realization accepts only expression controls")
        lane = arpeggiator.ExpressionLane(control.port.value)
        policy = self.profile.body.expression
        if policy.lanes.get(lane, policy.source) != "envelfo":
            raise ValueError(f"{lane} is not owned by Envelfo")
        return lane, control.value, envelfo_message(lane, control.value)

    def _note_messages(self, events: list[LiveEvent]) -> list[mido.Message]:
        output: list[mido.Message] = []
        for event in events:
            if event.kind == "on":
                if self.output_key is not None:
                    output.append(mido.Message.from_bytes([0x80, self.output_key, 0]))
                    self._end_note(event.at, 0)
                    self.output_id = None
                    self.output_key = None
                if not self._start_note(
                    event.at, f"input:{event.source_id}", event.key, event.velocity
                ):
                    continue
                self.tuning_offset = event.tuning_semitones
                entry = entry_messages(
                    self.profile.body.expression,
                    self.expression,
                    self.envelfo_expression,
                )
                if self.profile.body.tuning is not None and not any(
                    d[0] == 0xE0 for d in entry
                ):
                    entry.insert(0, [0xE0, 0, 64])
                output.extend(
                    mido.Message.from_bytes(
                        tuning_message(
                            d, self.tuning_offset, self.profile.body.bend_range
                        )
                    )
                    for d in entry
                )
                output.append(
                    mido.Message.from_bytes([0x90, event.key, event.velocity])
                )
                self.output_id = event.id
                self.output_key = event.key
            elif event.id == self.output_id:
                output.append(
                    mido.Message.from_bytes([0x80, event.key, event.velocity])
                )
                self._end_note(event.at, event.velocity)
                self.output_id = None
                self.output_key = None
        return output

    def _history_messages(self, events: list[RealizedMidiEvent]) -> list[mido.Message]:
        output: list[mido.Message] = []
        initialized: set[tuple[Fraction, str | None]] = set()
        attacks = {
            (e.at, e.source_note): e
            for e in events
            if e.data[0] == 0x90 and e.data[2] > 0
        }
        for event in events:
            kind = event.data[0] & 0xF0
            group = (event.at, event.source_note)
            if (
                group in attacks
                and group not in initialized
                and not (kind == 0x80 or kind == 0x90 and event.data[2] == 0)
            ):
                attack = attacks[group]
                assert attack.source_note is not None
                self._start_note(
                    attack.at, attack.source_note, attack.data[1], attack.data[2]
                )
                initialized.add(group)
                if self.suppressed_note:
                    continue
                self.tuning_offset = attack.tuning_semitones
                entry = entry_messages(
                    self.profile.body.expression,
                    self.expression,
                    self.envelfo_expression,
                )
                if self.profile.body.tuning is not None and not any(
                    d[0] == 0xE0 for d in entry
                ):
                    entry.insert(0, [0xE0, 0, 64])
                output.extend(
                    mido.Message.from_bytes(
                        tuning_message(
                            d, self.tuning_offset, self.profile.body.bend_range
                        )
                    )
                    for d in entry
                )
            if self.suppressed_note:
                continue
            if (lane := expression_lane(event.data)) is not None:
                policy = self.profile.body.expression
                if policy.lanes.get(lane, policy.source) != "recorded":
                    continue
            if kind == 0x90 and event.data[2] > 0:
                self.output_key = event.data[1]
            elif kind == 0x80 or kind == 0x90 and event.data[2] == 0:
                self._end_note(event.at, event.data[2])
                self.output_key = None
            output.append(
                mido.Message.from_bytes(
                    tuning_message(
                        event.data, self.tuning_offset, self.profile.body.bend_range
                    )
                )
            )
        return output

    def _elapsed(self, at_ns: int) -> int:
        if self.origin_ns is None:
            self.origin_ns = at_ns
            self.clock.anchor_us = 0
        self.wall_us = max(self.wall_us, (at_ns - self.origin_ns) // 1000)
        return self.wall_us


def main() -> None:
    command = tyro.cli(
        Annotated[ListPorts, tyro.conf.subcommand(name="list-ports")]
        | Annotated[Play, tyro.conf.subcommand(name="play")]
    )
    try:
        command.run()
    except (ValueError, OSError, RuntimeError, ImportError) as error:
        exit(str(error))


def _read_commands(commands: Inbox[str]) -> None:
    while True:
        line = stdin.readline(commands.max_message_bytes + 1)
        size = len(line.encode())
        if size > commands.max_message_bytes:
            commands.put("", size)
            return
        command = line.strip()
        if command in ("", "quit"):
            commands.put("quit", 4)
            return
        if command in (
            "clear",
            "start",
            "pause",
            "continue",
            "record",
            "commit",
            "overdub",
            "undo",
        ) or (
            command.startswith(
                (
                    "tempo ",
                    "gate ",
                    "density ",
                    "transposition ",
                    "selection_offset ",
                    "breath ",
                    "bend ",
                    "pressure ",
                )
            )
            and len(command.split()) == 2
        ):
            commands.put(command, size)
            if commands.failure.reason is not None:
                return
        else:
            print(
                "enter start, pause, continue, tempo BPM, gate FRACTION, "
                "density FRACTION, transposition SEMITONES, selection_offset RANKS, "
                "breath FRACTION, bend FRACTION, pressure FRACTION, "
                "record, commit, "
                "overdub, undo, clear, or quit",
                file=stderr,
            )
