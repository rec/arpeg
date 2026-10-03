"""Realize completed MIDI gestures with their captured controller state."""

from fractions import Fraction
from typing import Literal, Self

from pydantic import BaseModel, Field, model_validator
from ufor.arpeggiator_capture import CapturedPhrase
from ufor.events import MidiEvent

from .capture import MidiCaptureProfile


class MidiPlacement(BaseModel, frozen=True):
    note_id: str
    onset: Fraction = Field(ge=0)
    gate: Fraction | None = Field(default=None, ge=0)


class RealizedMidiEvent(BaseModel, frozen=True):
    at: Fraction
    data: list[int]
    source_note: str | None
    source_event: int | None


class MidiGestureRenderer(BaseModel, frozen=True):
    """Render completed notes onto independently allocated MIDI channels."""

    phrase: CapturedPhrase
    channels: list[int] = Field(min_length=1)
    timing: Literal["original", "fit"] = "original"
    expression_source: Literal["recorded", "current"] = "recorded"
    live_profile: MidiCaptureProfile = MidiCaptureProfile()
    live_events: list[MidiEvent] = Field(default_factory=list)

    @model_validator(mode="after")
    def valid_channels(self) -> Self:
        if len(self.channels) != len(set(self.channels)) or any(
            not 0 <= c <= 15 for c in self.channels
        ):
            raise ValueError("MIDI channels must be unique values from 0 to 15")
        if any(
            (b.tick, b.ordinal) <= (a.tick, a.ordinal)
            for a, b in zip(self.live_events, self.live_events[1:], strict=False)
        ):
            raise ValueError("live events must increase in (tick, ordinal) order")
        return self

    def render(self, placements: list[MidiPlacement]) -> list[RealizedMidiEvent]:
        """Allocate every simultaneous gesture or reject channel exhaustion."""
        notes = {n.note_id: n for n in self.phrase.notes}
        if any(p.note_id not in notes for p in placements):
            raise ValueError("placement references an unknown source note")
        if any(
            b.onset < a.onset for a, b in zip(placements, placements[1:], strict=False)
        ):
            raise ValueError("placements must be ordered by onset")
        reservations: list[_Reservation] = []
        events: list[tuple[Fraction, int, int, RealizedMidiEvent]] = []
        serial = 0
        for placement in placements:
            note = notes[placement.note_id]
            if note.key is None:
                raise ValueError("MIDI gesture requires a source key")
            gate = Fraction(note.gate_end_tick - note.onset_tick)
            output_gate = placement.gate if placement.gate is not None else gate
            if self.timing == "original" and output_gate != gate:
                raise ValueError("original timing requires the source gate")
            scale = output_gate / gate if self.timing == "fit" and gate else Fraction(1)
            control_times = (
                [
                    placement.onset
                    + (self.phrase.events[i].tick - note.onset_tick) * scale
                    for i in note.expression_events
                ]
                if self.expression_source == "recorded"
                else []
            )
            available = [
                c
                for c in self.channels
                if all(
                    r.channel != c
                    or r.gate_end <= placement.onset
                    and r.last_control < placement.onset
                    for r in reservations
                )
            ]
            if not available:
                raise ValueError("no MIDI channel is free for independent expression")
            channel = available[0]
            reservations.append(
                _Reservation(
                    channel=channel,
                    gate_end=placement.onset + output_gate,
                    last_control=max(control_times, default=Fraction(-1)),
                )
            )

            def add(
                at: Fraction,
                phase: int,
                data: list[int],
                source: int | None,
                channel: int = channel,
                note_id: str = note.note_id,
            ) -> None:
                nonlocal serial
                events.append(
                    (
                        at,
                        phase,
                        serial,
                        RealizedMidiEvent(
                            at=at,
                            data=[(data[0] & 0xF0) | channel, *data[1:]],
                            source_note=note_id,
                            source_event=source,
                        ),
                    )
                )
                serial += 1

            if self.expression_source == "recorded":
                for state in sorted(
                    note.entry_state.values(),
                    key=lambda s: s.source_event if s.source_event is not None else -1,
                ):
                    if state.source_event is not None:
                        source = self._midi(state.source_event)
                        add(placement.onset, 1, source.data, state.source_event)
            else:
                for lane in ("breath", "bend"):
                    prior = next(
                        (
                            e
                            for e in reversed(self.live_events)
                            if e.tick < placement.onset and self._live_lane(e) == lane
                        ),
                        None,
                    )
                    if prior is not None:
                        add(placement.onset, 1, prior.data, None)
                for event in self.live_events:
                    if event.tick == placement.onset and self._live_lane(event):
                        add(placement.onset, 1, event.data, None)
            if note.onset_event is None:
                raise ValueError("MIDI gesture requires a source note-on")
            add(placement.onset, 2, self._midi(note.onset_event).data, note.onset_event)
            if self.expression_source == "recorded":
                for index, at in zip(
                    note.expression_events, control_times, strict=True
                ):
                    add(at, 3, self._midi(index).data, index)
            else:
                for event in self.live_events:
                    if (
                        placement.onset < event.tick < placement.onset + output_gate
                        and self._live_lane(event)
                    ):
                        add(Fraction(event.tick), 3, event.data, None)
            if note.release_event is None:
                release = [0x80 | channel, note.key, 0]
            else:
                release = self._midi(note.release_event).data
            add(placement.onset + output_gate, 0, release, note.release_event)
        events.sort(key=lambda e: e[:3])
        return [event for _, _, _, event in events]

    def reorder(self, note_ids: list[str]) -> list[RealizedMidiEvent]:
        """Play selected cells in source timing, carrying each cell's gap duration."""
        notes = {n.note_id: n for n in self.phrase.notes}
        placements: list[MidiPlacement] = []
        at = Fraction(0)
        for note_id in note_ids:
            if note_id not in notes:
                raise ValueError("placement references an unknown source note")
            note = notes[note_id]
            placements.append(MidiPlacement(note_id=note_id, onset=at))
            at += note.cell_end_tick - note.onset_tick
        return self.render(placements)

    def replay_original(self) -> list[RealizedMidiEvent]:
        """Preserve every MIDI event's source time, bytes, and ledger order."""
        if any(not isinstance(e, MidiEvent) for e in self.phrase.events):
            raise ValueError("original MIDI replay requires a MIDI-only ledger")
        return [
            RealizedMidiEvent(
                at=Fraction(event.tick),
                data=event.data.copy(),
                source_note=None,
                source_event=index,
            )
            for index, event in enumerate(self.phrase.events)
            if isinstance(event, MidiEvent)
        ]

    def _midi(self, index: int) -> MidiEvent:
        event = self.phrase.events[index]
        if not isinstance(event, MidiEvent) or not 0x80 <= event.data[0] < 0xF0:
            raise ValueError("MIDI gesture references a non-channel event")
        return event

    def _live_lane(self, event: MidiEvent) -> str | None:
        if len(event.data) != 3 or event.data[0] & 15 != self.live_profile.channel:
            return None
        kind = event.data[0] & 0xF0
        if kind == 0xB0 and event.data[1] == self.live_profile.breath_cc:
            return "breath"
        if kind == 0xE0 and self.live_profile.track_bend:
            return "bend"
        return None


class _Reservation(BaseModel, frozen=True):
    channel: int
    gate_end: Fraction
    last_control: Fraction
