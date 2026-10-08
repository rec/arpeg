"""Live history and phrase takes captured in microseconds and played in exact beats."""

from fractions import Fraction
from math import ceil, floor
from typing import Literal, Self

from pydantic import BaseModel, Field, model_validator
from ufor import arpeggiator_ports
from ufor.events import MidiEvent
from ufor.time import Timebase

from .bank import CaptureBank
from .capture import MidiCaptureProfile
from .chance import draw_below
from .gesture import MidiGestureRenderer, MidiPlacement, RealizedMidiEvent
from .ports import PerformancePorts


class LiveHistoryArpeggiator(BaseModel):
    """Play completed history notes or explicitly committed phrase gestures."""

    step: Fraction
    gate: Fraction = Fraction(4, 5)
    ports: PerformancePorts | None = None
    seed: int | None = None
    name: str = ""
    decision_count: int = 0
    step_index: int = 0
    bank: CaptureBank = Field(default_factory=lambda: CaptureBank(mode="history"))
    capture_profile: MidiCaptureProfile = Field(default_factory=MidiCaptureProfile)
    expression_source: Literal["recorded", "current"] = "recorded"
    next_step: Fraction = Fraction(0)
    queue: list[RealizedMidiEvent] = Field(default_factory=list)
    sounding_key: int | None = None
    sounding_source: str | None = None
    processed_to: Fraction = Fraction(0)
    capture_to: int = 0
    inclusive: bool = False
    input_events: int = 0
    next_capture_id: int = 0

    @model_validator(mode="after")
    def valid_history(self) -> Self:
        if self.step <= 0 or self.gate < 0:
            raise ValueError(
                "capture playback requires positive step and nonnegative gate"
            )
        if self.ports is None:
            self.ports = PerformancePorts(gate=self.gate, density=Fraction(1))
        if self.bank.mode == "history" and self.bank.recording is None:
            self.bank.record(
                "live",
                Timebase.model_validate(
                    {"name": "microseconds", "rate": {"numerator": 1_000_000}}
                ),
                self.capture_profile,
            )
        return self

    def accept(self, event: MidiEvent) -> None:
        if self.bank.mode == "phrase" and self.bank.recording is None:
            return
        if self.input_events >= 1_000_000:
            raise ValueError("live capture reached its event limit")
        if event.tick < self.capture_to or (
            event.tick == self.capture_to and self.inclusive
        ):
            raise ValueError("MIDI input arrived after its live output time")
        self.bank.accept(event)
        self.input_events += 1

    def before(self, at: Fraction, tick: int) -> list[RealizedMidiEvent]:
        return self._process(at, tick, inclusive=False)

    def advance(self, at: Fraction, tick: int) -> list[RealizedMidiEvent]:
        return self._process(at, tick, inclusive=True)

    def control(
        self, at: Fraction, tick: int, control: arpeggiator_ports.ArpeggiatorControl
    ) -> list[RealizedMidiEvent]:
        assert self.ports is not None
        self.ports.check_control(control, self.seed)
        if at < self.processed_to:
            raise ValueError("live time must not go backwards")
        events = self.before(at, tick) if at > self.processed_to else []
        self.ports.pending[control.port] = control.value
        return events

    def clear(self, at: Fraction, tick: int) -> list[RealizedMidiEvent]:
        events = (
            [] if at == self.processed_to and self.inclusive else self.before(at, tick)
        )
        self.queue.clear()
        if self.sounding_key is not None and self.sounding_source is not None:
            events.append(
                RealizedMidiEvent(
                    at=at,
                    data=[128, self.sounding_key, 0],
                    source_note=self.sounding_source,
                    source_event=None,
                )
            )
        self.sounding_key = None
        self.sounding_source = None
        if self.bank.mode == "history":
            self.bank.clear_history()
        else:
            self.bank.clear()
            self.input_events = 0
        return events

    def capture(self, command: str, at: Fraction, tick: int) -> list[RealizedMidiEvent]:
        if self.bank.mode != "phrase":
            raise ValueError("capture controls require a phrase bank")
        self.bank.check_control(command)
        events = self.before(at, tick) if at > self.processed_to else []
        if command == "record":
            self.bank.record(
                f"take-{self.next_capture_id}",
                Timebase.model_validate(
                    {"name": "microseconds", "rate": {"numerator": 1_000_000}}
                ),
                self.capture_profile,
            )
            self.next_capture_id += 1
        elif command == "undo":
            self.bank.undo_last_capture()
        else:
            self.bank.commit(tick, "overdub" if command == "overdub" else "replace")
        self.capture_to = tick
        self.inclusive = False
        return events

    def stop(self, at: Fraction, tick: int) -> list[RealizedMidiEvent]:
        events = self.clear(at, tick)
        self.bank.clear()
        return events

    def pause(self, at: Fraction, tick: int) -> list[RealizedMidiEvent]:
        self.queue.clear()
        output = []
        if self.sounding_key is not None:
            output.append(
                RealizedMidiEvent(
                    at=at,
                    data=[128, self.sounding_key, 0],
                    source_note=self.sounding_source,
                    source_event=None,
                )
            )
        self.sounding_key = None
        self.sounding_source = None
        self.processed_to = at
        self.capture_to = tick
        self.inclusive = True
        self.next_step = max(self.next_step, ceil(at / self.step) * self.step)
        self.step_index = ceil(self.next_step / self.step)
        return output

    def relocate(self, at: Fraction, tick: int) -> list[RealizedMidiEvent]:
        output = self.pause(at, tick)
        self.next_step = ceil(at / self.step) * self.step
        self.step_index = ceil(at / self.step)
        assert self.ports is not None
        self.ports.pending.clear()
        self.bank.last_selected = None
        self.inclusive = False
        return output

    def _process(
        self, through: Fraction, tick: int, *, inclusive: bool
    ) -> list[RealizedMidiEvent]:
        if through < self.processed_to or (
            through == self.processed_to and self.inclusive and not inclusive
        ):
            raise ValueError("live time must not go backwards")
        output: list[RealizedMidiEvent] = []
        while True:
            deadline = (
                min(self.next_step, self.queue[0].at) if self.queue else self.next_step
            )
            if deadline > through:
                break
            if deadline == through and not inclusive:
                if (
                    self.queue
                    and self.queue[0].at == through
                    and (
                        self.queue[0].data[0] & 0xF0 == 0x80
                        or self.queue[0].data[0] & 0xF0 == 0x90
                        and self.queue[0].data[2] == 0
                    )
                ):
                    output.append(self.queue.pop(0))
                    self.sounding_key = None
                    self.sounding_source = None
                break
            if self.next_step == deadline:
                source_at = (
                    tick
                    if through == self.processed_to
                    else self.capture_to
                    + (tick - self.capture_to)
                    * (deadline - self.processed_to)
                    / (through - self.processed_to)
                )
                output.extend(self._play_step(deadline, floor(source_at)))
                self.next_step += self.step
                self.step_index += 1
            else:
                event = self.queue.pop(0)
                kind = event.data[0] & 0xF0
                if kind == 0x90 and event.data[2] > 0:
                    self.sounding_key = event.data[1]
                    self.sounding_source = event.source_note
                elif kind == 0x80 or kind == 0x90 and event.data[2] == 0:
                    self.sounding_key = None
                    self.sounding_source = None
                output.append(event)
        self.capture_to = tick
        self.processed_to = through
        self.inclusive = inclusive
        return output

    def _play_step(self, at: Fraction, source_tick: int) -> list[RealizedMidiEvent]:
        self.bank.advance(source_tick)
        self.bank.publish_step()
        assert self.ports is not None
        if not self.ports.begin_step(at, self.step_index, self.bank.revision):
            return []
        if not self.bank.published:
            self.bank.last_selected = None
            self.ports.outcome(False)
            return []
        index = self.decision_count
        self.decision_count += 1
        density = self.ports.density
        allowed = density == 1
        if 0 < density < 1:
            assert self.seed is not None
            allowed = (
                draw_below(
                    self.seed,
                    self.name,
                    "probability",
                    self.bank.revision,
                    index,
                    density.denominator,
                )
                < density.numerator
            )
        if not allowed:
            self.ports.outcome(False)
            return []
        selected = self.bank.select_step()
        if selected is None:
            return []
        phrase = self.bank.source(selected.capture_id)
        note = next(n for n in phrase.notes if n.note_id == selected.note_id)
        assert note.key is not None
        if (key := self.ports.realize_pitch(note.key)) is None:
            self.ports.outcome(False)
            return []
        self.ports.outcome(True)
        self.queue.clear()
        output: list[RealizedMidiEvent] = []
        if self.sounding_key is not None and self.sounding_source is not None:
            output.append(
                RealizedMidiEvent(
                    at=at,
                    data=[128, self.sounding_key, 0],
                    source_note=self.sounding_source,
                    source_event=None,
                )
            )
            self.sounding_key = None
            self.sounding_source = None
        self.queue.extend(
            MidiGestureRenderer(
                phrase=phrase,
                channels=[0],
                timing="fit",
                overlap="handoff",
                expression_source=self.expression_source,
            ).render(
                [
                    MidiPlacement(
                        note_id=selected.note_id,
                        onset=at,
                        gate=self.step * self.ports.gate,
                    )
                ]
            )
        )
        self.queue = [
            e.model_copy(
                update={
                    "source_note": f"{selected.capture_id}:{e.source_note}",
                    "data": [e.data[0], key, *e.data[2:]]
                    if e.data[0] & 0xF0 in (0x80, 0x90)
                    else e.data,
                }
            )
            for e in self.queue
        ]
        return output
