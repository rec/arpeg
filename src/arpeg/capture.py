"""Capture MIDI note gestures without changing the source event ledger."""

from __future__ import annotations

from typing import Literal, Self

from pydantic import BaseModel, Field, model_validator
from ufor.arpeggiator_capture import CapturedPhrase, EntryState, SourceNote
from ufor.events import MidiEvent
from ufor.time import Timebase


class MidiCaptureProfile(BaseModel, frozen=True):
    """Interpret a MIDI channel's notes and channel expression."""

    channel: int = Field(default=0, ge=0, le=15)
    overlap: Literal["handoff", "independent"] = "handoff"
    breath_cc: int = Field(default=2, ge=0, le=127)
    track_bend: bool = True
    tail_ticks: int = Field(default=0, ge=0)


class MidiCapture(BaseModel):
    """Record one explicitly delimited phrase in native source ticks."""

    capture_id: str
    timebase: Timebase
    profile: MidiCaptureProfile = Field(default_factory=MidiCaptureProfile, frozen=True)
    events: list[MidiEvent] = Field(default_factory=list)
    segments: list[_Segment] = Field(default_factory=list)
    prefix_events: list[int] = Field(default_factory=list)
    controller_state: dict[str, EntryState] = Field(default_factory=dict)
    active: list[int] = Field(default_factory=list)
    reported_notes: list[str] = Field(default_factory=list)
    advanced_through: int | None = None
    closed: bool = False

    @model_validator(mode="after")
    def initial_state(self) -> Self:
        if not self.controller_state:
            self.controller_state = {"breath": EntryState()}
            if self.profile.track_bend:
                self.controller_state["bend"] = EntryState()
        return self

    def accept(self, event: MidiEvent, note_id: str | None = None) -> None:
        """Append one wire event in (tick, ordinal) order."""
        if self.closed:
            raise ValueError("capture has ended")
        if self.advanced_through is not None and event.tick <= self.advanced_through:
            raise ValueError("MIDI event arrived after its capture time was published")
        if (
            event.tick < 0
            or self.events
            and (
                event.tick,
                event.ordinal,
            )
            <= (self.events[-1].tick, self.events[-1].ordinal)
        ):
            raise ValueError("capture events must increase in (tick, ordinal) order")
        status = event.data[0]
        kind = status & 0xF0
        onset = (
            0x80 <= status < 0xF0
            and status & 15 == self.profile.channel
            and kind == 0x90
            and len(event.data) == 3
            and event.data[2] > 0
        )
        if onset:
            if note_id is None:
                note_id = f"note-{len(self.segments)}"
            if any(n.note_id == note_id for n in self.segments):
                raise ValueError("duplicate source note ID")
            if (
                self.profile.overlap == "handoff"
                and self.segments
                and self.segments[-1].onset_tick == event.tick
            ):
                raise ValueError("simultaneous onsets require independent overlap")
        index = len(self.events)
        self.events.append(event)
        if not 0x80 <= status < 0xF0 or status & 15 != self.profile.channel:
            self._retain_context(index)
            return
        if len(event.data) != 3:
            self._retain_context(index)
            return
        _, first, value = event.data
        if onset:
            assert note_id is not None
            self._onset(index, first, value, note_id)
        elif kind == 0x80 or kind == 0x90 and value == 0:
            self._release(index, first, value)
        elif kind == 0xB0 and first == self.profile.breath_cc:
            self.controller_state["breath"] = EntryState(
                value=value / 127, source_event=index
            )
            self._expression_or_context(index)
        elif kind == 0xE0 and self.profile.track_bend:
            bend = (first | value << 7) - 8192
            self.controller_state["bend"] = EntryState(
                value=bend / 8192, source_event=index
            )
            self._expression_or_context(index)
        else:
            self._retain_context(index)

    def finish(self, end_tick: int) -> CapturedPhrase:
        """End open gates and trailing cells at the declared phrase boundary."""
        if self.closed:
            raise ValueError("capture has ended")
        if (
            end_tick < 0
            or self.events
            and end_tick < self.events[-1].tick
            or self.advanced_through is not None
            and end_tick < self.advanced_through
        ):
            raise ValueError("phrase end precedes source events")
        for segment in self.segments:
            if segment.gate_end_tick is None:
                segment.gate_end_tick = end_tick
            if segment.cell_end_tick is None:
                segment.cell_end_tick = end_tick
        phrase = CapturedPhrase(
            capture_id=self.capture_id,
            timebase=self.timebase,
            end_tick=end_tick,
            events=self.events,
            prefix_events=self.prefix_events,
            notes=[self._source_note(n) for n in self.segments],
        )
        self.closed = True
        return phrase

    def advance(self, through_tick: int) -> list[SourceNote]:
        """Publish newly complete cells after their declared capture tail."""
        if self.closed:
            raise ValueError("capture has ended")
        if (
            through_tick < 0
            or self.events
            and through_tick < self.events[-1].tick
            or self.advanced_through is not None
            and through_tick < self.advanced_through
        ):
            raise ValueError("capture time precedes source events")
        self.advanced_through = through_tick
        ready: list[SourceNote] = []
        for index, segment in enumerate(self.segments):
            gate = segment.gate_end_tick
            if gate is None or through_tick < gate + self.profile.tail_ticks:
                continue
            if segment.cell_end_tick is None and index not in self.active:
                segment.cell_end_tick = max(
                    gate + self.profile.tail_ticks, segment.onset_tick + 1
                )
            if (
                segment.cell_end_tick is not None
                and segment.note_id not in self.reported_notes
            ):
                ready.append(self._source_note(segment))
                self.reported_notes.append(segment.note_id)
        return ready

    def snapshot(self, through_tick: int) -> CapturedPhrase:
        """Expose complete notes and the current source ledger while recording."""
        self.advance(through_tick)
        return CapturedPhrase(
            capture_id=self.capture_id,
            timebase=self.timebase,
            end_tick=through_tick,
            events=self.events,
            notes=[
                self._source_note(n)
                for n in self.segments
                if n.note_id in self.reported_notes
            ],
            prefix_events=self.prefix_events,
        )

    def _onset(self, index: int, key: int, velocity: int, note_id: str) -> None:
        tick = self.events[index].tick
        if self.profile.overlap == "handoff":
            for active in self.active:
                segment = self.segments[active]
                segment.gate_end_tick = tick
            self.active.clear()
            if self.segments and self.segments[-1].cell_end_tick is None:
                self.segments[-1].cell_end_tick = tick
        self.segments.append(
            _Segment(
                note_id=note_id,
                onset_tick=tick,
                onset_event=index,
                key=key,
                velocity=velocity,
                entry_state=self.controller_state.copy(),
            )
        )
        self.active.append(len(self.segments) - 1)

    def _release(self, index: int, key: int, velocity: int) -> None:
        for active in self.active:
            segment = self.segments[active]
            if segment.key == key:
                segment.gate_end_tick = self.events[index].tick
                segment.release_event = index
                segment.release_velocity = velocity
                self.active.remove(active)
                return
        self._retain_context(index)

    def _expression_or_context(self, index: int) -> None:
        if self.active:
            for active in self.active:
                self.segments[active].expression_events.append(index)
        elif self.segments:
            self._retain_context(index)

    def _retain_context(self, index: int) -> None:
        if not self.segments:
            self.prefix_events.append(index)
        elif not self.active and self.segments[-1].cell_end_tick is None:
            self.segments[-1].following_events.append(index)

    def _source_note(self, segment: _Segment) -> SourceNote:
        assert segment.gate_end_tick is not None
        assert segment.cell_end_tick is not None
        return SourceNote(
            capture_id=self.capture_id,
            note_id=segment.note_id,
            onset_tick=segment.onset_tick,
            gate_end_tick=segment.gate_end_tick,
            cell_end_tick=segment.cell_end_tick,
            onset_event=segment.onset_event,
            release_event=segment.release_event,
            expression_events=segment.expression_events,
            following_events=segment.following_events,
            entry_state=segment.entry_state,
            key=segment.key,
            velocity=segment.velocity / 127,
            release_velocity=(
                segment.release_velocity / 127
                if segment.release_velocity is not None
                else None
            ),
        )


class _Segment(BaseModel):
    note_id: str
    onset_tick: int
    onset_event: int
    key: int
    velocity: int
    entry_state: dict[str, EntryState]
    gate_end_tick: int | None = None
    cell_end_tick: int | None = None
    release_event: int | None = None
    release_velocity: int | None = None
    expression_events: list[int] = Field(default_factory=list)
    following_events: list[int] = Field(default_factory=list)
