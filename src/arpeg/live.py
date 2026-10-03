"""Incremental held-note arpeggiation without MIDI or audio devices.

Feed note changes in beat order, then call ``advance`` to play through a beat.
Changes at the same beat are applied before that beat's arpeggio step.
"""

from fractions import Fraction
from typing import Literal

from pydantic import BaseModel, Field
from ufor.arpeggiator import (
    ArpeggiatorScore,
    Ascending,
    Descending,
    Grid,
    HeldBank,
    Played,
)


class LiveEvent(BaseModel, frozen=True):
    at: Fraction
    kind: Literal["on", "off"]
    id: int
    key: int = Field(ge=0, le=127)
    velocity: int = Field(ge=0, le=127)


class LiveArpeggiator:
    """Turn a changing held-note bank into ordered note events."""

    def __init__(self, profile: ArpeggiatorScore) -> None:
        body = profile.body
        if not isinstance(body.bank, HeldBank):
            raise ValueError("live mode currently supports held banks only")
        if not isinstance(body.selection, (Ascending, Descending, Played)):
            raise ValueError("live mode requires a classic note selection")
        if (
            isinstance(body.selection, (Ascending, Descending))
            and body.selection.key != "pitch"
        ):
            raise ValueError("live mode requires pitch selection")
        if isinstance(body.selection, Ascending) and body.selection.repeats != 1:
            raise ValueError("live mode does not support repeated selections")
        if not isinstance(body.rhythm, Grid):
            raise ValueError("live mode requires grid rhythm")

        self.selection = body.selection
        self.step = Fraction(body.rhythm.step.removesuffix(" beat"))
        self.gate = body.gate
        self.next_step = Fraction(0)
        self.now = Fraction(0)
        self.next_input_id = 0
        self.next_output_id = 0
        self.previous_key: tuple[Fraction, int] | None = None
        self.input: list[_InputNote] = []
        self.sounding: list[_SoundingNote] = []

    def note_on(self, at: Fraction, key: int, velocity: int) -> list[LiveEvent]:
        """Add a note, leaving the step at ``at`` for ``advance``."""
        self._check_time(at)
        note = _InputNote(id=self.next_input_id, key=key, velocity=velocity, onset=at)
        events = self._process_until(at, inclusive=False)
        self.input.append(note)
        self.next_input_id += 1
        return events

    def note_off(self, at: Fraction, key: int) -> list[LiveEvent]:
        """Release the oldest held input note with this pitch."""
        self._check_time(at)
        if not any(n.key == key for n in self.input):
            raise ValueError("release has no matching onset")
        events = self._process_until(at, inclusive=False)
        self.input.pop(next(i for i, n in enumerate(self.input) if n.key == key))
        if not self.input:
            self.previous_key = None
            events.extend(self._release_all(at))
        return events

    def advance(self, through: Fraction) -> list[LiveEvent]:
        """Emit all scheduled events through this beat, inclusive."""
        self._check_time(through)
        return self._process_until(through, inclusive=True)

    def stop(self, at: Fraction) -> list[LiveEvent]:
        """Release only this arpeggiator's sounding notes and clear its bank."""
        self._check_time(at)
        events = self._process_until(at, inclusive=False)
        events.extend(self._release_all(at))
        self.input.clear()
        self.previous_key = None
        self.now = at
        return events

    def next_deadline(self) -> Fraction:
        """Return the next step or owned release beat."""
        return min([self.next_step, *(n.end for n in self.sounding)])

    def _check_time(self, at: Fraction) -> None:
        if at < self.now:
            raise ValueError("live time must not go backwards")

    def _process_until(self, through: Fraction, *, inclusive: bool) -> list[LiveEvent]:
        events: list[LiveEvent] = []
        while (at := self.next_deadline()) < through or (inclusive and at == through):
            remaining: list[_SoundingNote] = []
            for note in self.sounding:
                if note.end <= at:
                    events.append(
                        LiveEvent(
                            at=note.end,
                            kind="off",
                            id=note.id,
                            key=note.key,
                            velocity=0,
                        )
                    )
                else:
                    remaining.append(note)
            self.sounding = remaining
            if self.next_step == at:
                events.extend(self._play_step(at))
                self.next_step += self.step
        self.now = through
        return events

    def _play_step(self, at: Fraction) -> list[LiveEvent]:
        if not self.input:
            self.previous_key = None
            return []
        ordered = sorted(self.input, key=self._selection_key)
        note = next(
            (
                n
                for n in ordered
                if self.previous_key is None
                or self._selection_key(n) > self.previous_key
            ),
            ordered[0],
        )
        self.previous_key = self._selection_key(note)
        events: list[LiveEvent] = []
        remaining: list[_SoundingNote] = []
        for sounding in self.sounding:
            if sounding.key == note.key:
                events.append(
                    LiveEvent(
                        at=at, kind="off", id=sounding.id, key=sounding.key, velocity=0
                    )
                )
            else:
                remaining.append(sounding)
        self.sounding = remaining
        output_id = self.next_output_id
        self.next_output_id += 1
        events.append(
            LiveEvent(
                at=at, kind="on", id=output_id, key=note.key, velocity=note.velocity
            )
        )
        if (end := at + self.step * self.gate) == at:
            events.append(
                LiveEvent(at=at, kind="off", id=output_id, key=note.key, velocity=0)
            )
        else:
            self.sounding.append(_SoundingNote(id=output_id, key=note.key, end=end))
        return events

    def _release_all(self, at: Fraction) -> list[LiveEvent]:
        events = [
            LiveEvent(at=at, kind="off", id=n.id, key=n.key, velocity=0)
            for n in self.sounding
        ]
        self.sounding.clear()
        return events

    def _selection_key(self, note: "_InputNote") -> tuple[Fraction, int]:
        selection = self.selection
        if isinstance(selection, Played):
            position = -note.onset if selection.direction == "reverse" else note.onset
        else:
            position = Fraction(
                -note.key if isinstance(selection, Descending) else note.key
            )
        return position, note.id


class _InputNote(BaseModel, frozen=True):
    id: int
    key: int = Field(ge=0, le=127)
    velocity: int = Field(ge=1, le=127)
    onset: Fraction


class _SoundingNote(BaseModel, frozen=True):
    id: int
    key: int
    end: Fraction
