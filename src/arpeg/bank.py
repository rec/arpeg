"""Publish completed MIDI notes as history or committed phrase revisions."""

from __future__ import annotations

from typing import Literal

from pydantic import BaseModel, Field
from ufor.arpeggiator_capture import CapturedPhrase
from ufor.events import MidiEvent
from ufor.time import Timebase

from .capture import MidiCapture, MidiCaptureProfile


class BankNote(BaseModel, frozen=True):
    capture_id: str
    note_id: str


class CaptureBank(BaseModel):
    """Record takes, then publish bank changes at the caller's step boundary."""

    mode: Literal["history", "phrase"]
    history_size: int = Field(default=8, ge=1)
    selection: Literal["ascending", "descending", "played"] = "ascending"
    direction: Literal["forward", "reverse"] = "forward"
    retrigger_on_edit: bool = True
    recording: MidiCapture | None = None
    live_snapshot: CapturedPhrase | None = None
    takes: list[_Take] = Field(default_factory=list)
    published: list[BankNote] = Field(default_factory=list)
    revision: int = 0
    last_selected: BankNote | None = None
    selected_revision: int = -1
    history_floor: int = 0

    def record(
        self, capture_id: str, timebase: Timebase, profile: MidiCaptureProfile
    ) -> None:
        if self.recording is not None:
            raise ValueError("a capture is already recording")
        if any(t.phrase.capture_id == capture_id for t in self.takes):
            raise ValueError("capture ID already exists")
        self.recording = MidiCapture(
            capture_id=capture_id, timebase=timebase, profile=profile
        )
        self.live_snapshot = None
        self.history_floor = 0

    def accept(self, event: MidiEvent, note_id: str | None = None) -> None:
        if self.recording is None:
            raise ValueError("record before accepting MIDI")
        self.recording.accept(event, note_id)

    def advance(self, through_tick: int) -> None:
        """Make completed history notes eligible after their capture tail."""
        if self.mode == "history" and self.recording is not None:
            self.live_snapshot = self.recording.snapshot(through_tick)

    def commit(
        self, end_tick: int, update: Literal["replace", "overdub"] | None = None
    ) -> None:
        if self.recording is None:
            raise ValueError("record before committing")
        phrase = self.recording.finish(end_tick)
        if update is None:
            update = "overdub" if self.mode == "history" else "replace"
        self.takes.append(_Take(phrase=phrase, update=update))
        self.recording = None
        self.live_snapshot = None

    def undo_last_capture(self) -> None:
        if not self.takes:
            raise ValueError("there is no committed capture to undo")
        self.takes.pop()

    def clear(self) -> None:
        self.recording = None
        self.live_snapshot = None
        self.takes.clear()

    def clear_history(self) -> None:
        """Forget selected history while preserving the live controller capture."""
        if self.mode != "history" or self.recording is None:
            raise ValueError("clear_history requires an active history capture")
        self.takes.clear()
        self.history_floor = len(self.recording.segments)
        self.published.clear()
        self.last_selected = None
        self.revision += 1

    def publish_step(self) -> list[BankNote]:
        """Apply pending edits once at the next rhythm opportunity."""
        notes: list[BankNote] = []
        for take in self.takes:
            if take.update == "replace":
                notes.clear()
            notes.extend(
                BankNote(capture_id=take.phrase.capture_id, note_id=n.note_id)
                for n in take.phrase.notes
            )
        if self.mode == "history":
            if self.live_snapshot is not None:
                notes.extend(
                    BankNote(
                        capture_id=self.live_snapshot.capture_id, note_id=n.note_id
                    )
                    for n in self.live_snapshot.notes[self.history_floor :]
                )
            notes = notes[-self.history_size :]
        if notes != self.published:
            self.published = notes
            self.revision += 1
        return self.published.copy()

    def select_step(self) -> BankNote | None:
        """Publish changes and choose one identified note for this step."""
        bank = self.publish_step()
        if not bank:
            self.last_selected = None
            return None
        if self.selected_revision != self.revision:
            if self.retrigger_on_edit:
                self.last_selected = None
            self.selected_revision = self.revision
        if self.selection == "played":
            ordered = bank if self.direction == "forward" else list(reversed(bank))
        else:

            def order(ref: BankNote) -> tuple[int, str, str]:
                note = next(
                    n
                    for n in self.source(ref.capture_id).notes
                    if n.note_id == ref.note_id
                )
                if note.key is None:
                    raise ValueError("MIDI selection requires a source key")
                key = note.key if self.selection == "ascending" else -note.key
                return (key, ref.capture_id, ref.note_id)

            ordered = sorted(bank, key=order)
        if self.last_selected in ordered:
            index = ordered.index(self.last_selected) + 1
        else:
            index = 0
        self.last_selected = ordered[index % len(ordered)]
        return self.last_selected

    def source(self, capture_id: str) -> CapturedPhrase:
        if (
            self.live_snapshot is not None
            and self.live_snapshot.capture_id == capture_id
        ):
            return self.live_snapshot
        for take in self.takes:
            if take.phrase.capture_id == capture_id:
                return take.phrase
        raise ValueError("source capture is not in the bank")


class _Take(BaseModel, frozen=True):
    phrase: CapturedPhrase
    update: Literal["replace", "overdub"]
