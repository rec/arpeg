"""Live history selection and owned MIDI output in exact source ticks."""

from fractions import Fraction
from math import ceil, floor
from typing import Self

from pydantic import BaseModel, Field, model_validator
from ufor.events import MidiEvent
from ufor.time import Timebase

from .bank import CaptureBank
from .capture import MidiCaptureProfile
from .gesture import MidiGestureRenderer, MidiPlacement, RealizedMidiEvent


class LiveHistoryArpeggiator(BaseModel):
    """Publish completed notes at steps and fit their recorded gestures."""

    step: Fraction
    gate: Fraction = Fraction(4, 5)
    bank: CaptureBank = Field(default_factory=lambda: CaptureBank(mode="history"))
    capture_profile: MidiCaptureProfile = Field(default_factory=MidiCaptureProfile)
    next_step: Fraction = Fraction(0)
    queue: list[RealizedMidiEvent] = Field(default_factory=list)
    sounding_key: int | None = None
    sounding_source: str | None = None
    processed_to: Fraction = Fraction(0)
    capture_to: int = 0
    inclusive: bool = False
    input_events: int = 0

    @model_validator(mode="after")
    def valid_history(self) -> Self:
        if self.step <= 0 or self.gate < 0 or self.bank.mode != "history":
            raise ValueError("history requires positive step and nonnegative gate")
        if self.bank.recording is None:
            self.bank.record(
                "live",
                Timebase.model_validate(
                    {"name": "microseconds", "rate": {"numerator": 1_000_000}}
                ),
                self.capture_profile,
            )
        return self

    def accept(self, event: MidiEvent) -> None:
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
        self.bank.clear_history()
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
        return output

    def relocate(self, at: Fraction, tick: int) -> list[RealizedMidiEvent]:
        output = self.pause(at, tick)
        self.next_step = ceil(at / self.step) * self.step
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
            if deadline > through or (deadline == through and not inclusive):
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
        selected = self.bank.select_step()
        if selected is None:
            return []
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
                phrase=self.bank.source(selected.capture_id),
                channels=[0],
                timing="fit",
                overlap="handoff",
            ).render(
                [
                    MidiPlacement(
                        note_id=selected.note_id, onset=at, gate=self.step * self.gate
                    )
                ]
            )
        )
        return output
