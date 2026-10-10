"""Exact host transport with 24-pulse MIDI clock acquisition."""

from enum import StrEnum, auto
from fractions import Fraction
from typing import Self

from pydantic import BaseModel, Field, model_validator


class ClockMode(StrEnum):
    internal = auto()
    external = auto()


class ClockObservation(BaseModel, frozen=True):
    at_us: int
    data: list[int]


class TransportClock(BaseModel):
    mode: ClockMode = ClockMode.internal
    bpm: int = Field(default=120, ge=1, le=1000)
    timeout_us: int = Field(default=500_000, gt=0)
    beat: Fraction = Fraction(0)
    anchor_beat: Fraction = Fraction(0)
    anchor_us: int | None = None
    running: bool = True
    last_pulse_us: int | None = None
    interval_us: int | None = None
    waiting: bool = True
    observations: list[ClockObservation] = Field(default_factory=list)

    @model_validator(mode="after")
    def wait_for_external_transport(self) -> Self:
        if self.mode == ClockMode.external and self.anchor_us is None:
            self.running = False
        return self

    @property
    def active(self) -> bool:
        return (
            self.running
            and self.anchor_us is not None
            and (self.mode == ClockMode.internal or not self.waiting)
        )

    def advance(self, at_us: int) -> Fraction:
        if self.active:
            assert self.anchor_us is not None
            elapsed = max(0, at_us - self.anchor_us)
            if self.mode == ClockMode.internal:
                self.beat = self.anchor_beat + Fraction(elapsed * self.bpm, 60_000_000)
            else:
                if self.interval_us is not None:
                    self.beat = max(
                        self.beat,
                        self.anchor_beat
                        + min(Fraction(elapsed, self.interval_us), Fraction(1)) / 24,
                    )
                assert self.last_pulse_us is not None
                if at_us - self.last_pulse_us >= self.timeout_us:
                    self.running = False
                    self.waiting = True
        return self.beat

    def accept(self, at_us: int, data: list[int]) -> bool:
        """Return whether Start or song position requests a playback relocation."""
        self.advance(at_us)
        if len(self.observations) >= 1_000_000:
            raise ValueError("clock observation limit reached")
        self.observations.append(ClockObservation(at_us=at_us, data=data))
        status = data[0]
        if status == 0xFA or status == 0xF2:
            self.beat = (
                Fraction(0) if status == 0xFA else Fraction(data[1] + 128 * data[2], 4)
            )
            self.anchor_beat = self.beat
            self.anchor_us = at_us
            self.last_pulse_us = None
            self.interval_us = None
            self.waiting = True
            if status == 0xFA:
                self.running = True
            return True
        if status == 0xFC:
            self.running = False
            self.waiting = True
        elif status == 0xFB:
            self.running = True
            self.anchor_us = at_us
            self.anchor_beat = self.beat
            self.last_pulse_us = None
            self.interval_us = None
            self.waiting = True
        elif status == 0xF8 and self.mode == ClockMode.external and self.running:
            if self.last_pulse_us is not None and at_us <= self.last_pulse_us:
                return False
            if self.waiting:
                self.anchor_beat = self.beat
                self.waiting = False
            else:
                assert self.last_pulse_us is not None
                self.interval_us = at_us - self.last_pulse_us
                self.anchor_beat += Fraction(1, 24)
                self.beat = max(self.beat, self.anchor_beat)
            self.last_pulse_us = at_us
            self.anchor_us = at_us
        return False

    def set_tempo(self, at_us: int, bpm: int) -> None:
        if not 1 <= bpm <= 1000 or self.mode != ClockMode.internal:
            raise ValueError("tempo requires internal clock and BPM between 1 and 1000")
        self.advance(at_us)
        self.anchor_us = at_us
        self.anchor_beat = self.beat
        self.bpm = bpm

    def halt(self, at_us: int) -> None:
        """Stop even when observation storage has reached its limit."""
        self.advance(at_us)
        self.running = False
        self.waiting = True
