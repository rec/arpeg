"""Incremental note arpeggiation without MIDI or audio devices.

Feed note changes in beat order, then call ``advance`` to play through a beat.
Changes at the same beat are applied before that beat's arpeggio step.
"""

from __future__ import annotations

from fractions import Fraction
from functools import cached_property
from math import ceil
from typing import Literal, Self

from pydantic import BaseModel, Field, model_validator
from ufor.arpeggiator import (
    Alternating,
    ArpeggiatorScore,
    Ascending,
    Choice,
    Descending,
    Euclidean,
    Grid,
    HeldBank,
    IndexPattern,
    InsideOut,
    LatchedBank,
    OutsideIn,
    Pattern,
    Played,
    Shuffle,
    Walk,
)

from .chance import draw_below
from .rhythm import RhythmDecision, decide_step


class LiveEvent(BaseModel, frozen=True):
    at: Fraction
    kind: Literal["on", "off"]
    id: int
    source_id: int
    key: int = Field(ge=0, le=127)
    velocity: int = Field(ge=0, le=127)


class LiveArpeggiator(BaseModel):
    """Turn changing input notes into ordered note events."""

    profile: ArpeggiatorScore = Field(frozen=True)
    next_step: Fraction = Fraction(0)
    step_index: int = 0
    bank_revision: int = 0
    decision_count: int = 0
    walk_count: int = 0
    walk_rank: int = 0
    pattern_position: int = 0
    shuffle_order: list[int] = Field(default_factory=list)
    shuffle_position: int = 0
    shuffle_revision: int = -1
    shuffle_count: int = 0
    choice_count: int = 0
    rising: bool = True
    pending: list[_Attack] = Field(default_factory=list)
    now: Fraction = Fraction(0)
    next_input_id: int = 0
    next_output_id: int = 0
    previous_note: _InputNote | None = None
    input: list[_InputNote] = Field(default_factory=list)
    bank: list[_InputNote] = Field(default_factory=list)
    toggle_at: Fraction | None = None
    toggled_keys: list[int] = Field(default_factory=list)
    toggle_added_keys: list[int] = Field(default_factory=list)
    sounding: list[_SoundingNote] = Field(default_factory=list)

    @model_validator(mode="after")
    def supported_profile(self) -> Self:
        body = self.profile.body
        if not isinstance(body.bank, (HeldBank, LatchedBank)):
            raise ValueError("live mode requires a held or latched bank")
        if not isinstance(
            body.selection,
            (
                Ascending,
                Descending,
                Played,
                Alternating,
                InsideOut,
                OutsideIn,
                IndexPattern,
                Shuffle,
                Choice,
                Walk,
            ),
        ):
            raise ValueError("unsupported live note selection")
        if (
            isinstance(body.selection, (Ascending, Descending))
            and body.selection.key != "pitch"
        ):
            raise ValueError("live mode requires pitch selection")
        if isinstance(body.selection, Ascending) and body.selection.repeats != 1:
            raise ValueError("live mode does not support repeated selections")
        if not isinstance(body.rhythm, (Grid, Euclidean, Pattern)):
            raise ValueError("live mode requires grid, Euclidean, or pattern rhythm")
        return self

    @cached_property
    def bank_mode(self) -> HeldBank | LatchedBank:
        bank = self.profile.body.bank
        assert isinstance(bank, (HeldBank, LatchedBank))
        return bank

    @cached_property
    def selection(
        self,
    ) -> (
        Ascending
        | Descending
        | Played
        | Alternating
        | InsideOut
        | OutsideIn
        | IndexPattern
        | Shuffle
        | Choice
        | Walk
    ):
        selection = self.profile.body.selection
        assert isinstance(
            selection,
            (
                Ascending,
                Descending,
                Played,
                Alternating,
                InsideOut,
                OutsideIn,
                IndexPattern,
                Shuffle,
                Choice,
                Walk,
            ),
        )
        return selection

    def note_on(self, at: Fraction, key: int, velocity: int) -> list[LiveEvent]:
        """Add a note, leaving the step at ``at`` for ``advance``."""
        self._check_time(at)
        note = _InputNote(id=self.next_input_id, key=key, velocity=velocity, onset=at)
        events = self._process_until(at, inclusive=False)
        new_chord = not self.input
        self.input.append(note)
        edited = isinstance(self.bank_mode, HeldBank)
        if isinstance(self.bank_mode, LatchedBank):
            if self.bank_mode.update == "replace":
                if new_chord:
                    self.bank.clear()
                self.bank.append(note)
                edited = True
            elif self.bank_mode.update == "add":
                self.bank.append(note)
                edited = True
            elif self.bank_mode.update == "toggle":
                if self.toggle_at != at:
                    self.toggle_at = at
                    self.toggled_keys.clear()
                    self.toggle_added_keys.clear()
                if key not in self.toggled_keys:
                    edited = True
                    self.toggled_keys.append(key)
                    if any(n.key == key for n in self.bank):
                        self.bank = [n for n in self.bank if n.key != key]
                    else:
                        self.bank.append(note)
                        self.toggle_added_keys.append(key)
                elif key in self.toggle_added_keys:
                    self.bank.append(note)
                    edited = True
            if not self.bank:
                self.previous_note = None
                self.pattern_position = 0
                self.shuffle_order.clear()
                self.shuffle_position = 0
                events.extend(self._release_all(at))
        if edited:
            self.bank_revision += 1
        if edited and self.profile.body.retrigger == "bank_edit":
            self.previous_note = None
            self.pattern_position = 0
            self.shuffle_order.clear()
            self.shuffle_position = 0
        self.next_input_id += 1
        return events

    def note_off(self, at: Fraction, key: int) -> list[LiveEvent]:
        """Release the oldest held input note with this pitch."""
        self._check_time(at)
        if not any(n.key == key for n in self.input):
            raise ValueError("release has no matching onset")
        events = self._process_until(at, inclusive=False)
        self.input.pop(next(i for i, n in enumerate(self.input) if n.key == key))
        if isinstance(self.bank_mode, HeldBank):
            self.bank_revision += 1
        if (
            isinstance(self.bank_mode, HeldBank)
            and self.profile.body.retrigger == "bank_edit"
        ):
            self.previous_note = None
            self.pattern_position = 0
            self.shuffle_order.clear()
            self.shuffle_position = 0
        if not self.input and isinstance(self.bank_mode, HeldBank):
            self.previous_note = None
            self.pattern_position = 0
            self.shuffle_order.clear()
            self.shuffle_position = 0
            events.extend(self._release_all(at))
        return events

    def advance(self, through: Fraction) -> list[LiveEvent]:
        """Emit all scheduled events through this beat, inclusive."""
        self._check_time(through)
        return self._process_until(through, inclusive=True)

    def before(self, through: Fraction) -> list[LiveEvent]:
        """Release due notes before expression, leaving this beat's attacks pending."""
        self._check_time(through)
        events = self._process_until(through, inclusive=False)
        events.extend(self._release_due(through))
        return events

    def stop(self, at: Fraction) -> list[LiveEvent]:
        """Release only this arpeggiator's sounding notes and clear its bank."""
        self._check_time(at)
        events = self._process_until(at, inclusive=False)
        events.extend(self._release_all(at))
        active = self.input if isinstance(self.bank_mode, HeldBank) else self.bank
        if active:
            self.bank_revision += 1
        self.input.clear()
        self.bank.clear()
        self.toggle_at = None
        self.toggled_keys.clear()
        self.toggle_added_keys.clear()
        self.previous_note = None
        self.pattern_position = 0
        self.shuffle_order.clear()
        self.shuffle_position = 0
        self.now = at
        return events

    def clear(self, at: Fraction) -> list[LiveEvent]:
        """Clear a latched bank and release its owned output notes."""
        self._check_time(at)
        if isinstance(self.bank_mode, HeldBank):
            raise ValueError("clear requires a latched bank")
        events = self._process_until(at, inclusive=False)
        if self.bank:
            self.bank_revision += 1
        self.bank.clear()
        self.toggle_at = None
        self.toggled_keys.clear()
        self.toggle_added_keys.clear()
        self.previous_note = None
        self.pattern_position = 0
        self.shuffle_order.clear()
        self.shuffle_position = 0
        events.extend(self._release_all(at))
        return events

    def next_deadline(self) -> Fraction:
        """Return the next step, pending attack, or owned release beat."""
        return min(
            [
                self.next_step,
                *(n.end for n in self.sounding),
                *(n.at for n in self.pending),
            ]
        )

    def pause(self, at: Fraction) -> list[LiveEvent]:
        """Cancel output while retaining input notes and selector position."""
        events = self._release_all(at)
        self.now = at
        rhythm = self.profile.body.rhythm
        assert isinstance(rhythm, (Grid, Euclidean, Pattern))
        while self.next_step < at:
            self.next_step += decide_step(
                rhythm, self.step_index, self.profile.body.gate
            ).duration
            self.step_index += 1
        return events

    def relocate(self, at: Fraction) -> list[LiveEvent]:
        """Seek the rhythm and restart traversal without forgetting held notes."""
        events = self._release_all(at)
        rhythm = self.profile.body.rhythm
        if isinstance(rhythm, Pattern):
            cycle = sum(
                (Fraction(s.duration.removesuffix(" beat")) for s in rhythm.steps),
                Fraction(0),
            )
            cycles = at // cycle
            self.next_step = cycles * cycle
            self.step_index = cycles * len(rhythm.steps)
            while self.next_step < at:
                self.next_step += decide_step(
                    rhythm, self.step_index, self.profile.body.gate
                ).duration
                self.step_index += 1
        else:
            assert isinstance(rhythm, (Grid, Euclidean))
            step = Fraction(rhythm.step.removesuffix(" beat"))
            self.step_index = ceil(at / step)
            self.next_step = self.step_index * step
        self.now = at
        self.previous_note = None
        self.rising = True
        self.pattern_position = 0
        self.shuffle_order.clear()
        self.shuffle_position = 0
        return events

    def _check_time(self, at: Fraction) -> None:
        if at < self.now:
            raise ValueError("live time must not go backwards")

    def _process_until(self, through: Fraction, *, inclusive: bool) -> list[LiveEvent]:
        events: list[LiveEvent] = []
        while (at := self.next_deadline()) < through or (inclusive and at == through):
            events.extend(self._release_due(at))
            if self.next_step == at:
                rhythm = self.profile.body.rhythm
                assert isinstance(rhythm, (Grid, Euclidean, Pattern))
                decision = decide_step(rhythm, self.step_index, self.profile.body.gate)
                self._schedule_step(at, decision)
                self.next_step += decision.duration
                self.step_index += 1
            while self.pending and self.pending[0].at == at:
                events.extend(self._attack(self.pending.pop(0)))
        self.now = through
        return events

    def _release_due(self, at: Fraction) -> list[LiveEvent]:
        events = [
            LiveEvent(
                at=n.end,
                kind="off",
                id=n.id,
                source_id=n.source_id,
                key=n.key,
                velocity=0,
            )
            for n in self.sounding
            if n.end <= at
        ]
        self.sounding = [n for n in self.sounding if n.end > at]
        return events

    def _schedule_step(self, at: Fraction, decision: RhythmDecision) -> None:
        active = self.input if isinstance(self.bank_mode, HeldBank) else self.bank
        if not active:
            self.previous_note = None
            self.pattern_position = 0
            self.shuffle_order.clear()
            self.shuffle_position = 0
            return
        if not decision.repeats:
            return
        body = self.profile.body
        decision_index = self.decision_count
        self.decision_count += 1
        if body.probability == 0:
            return
        if body.probability < 1:
            assert body.seed is not None
            if (
                draw_below(
                    body.seed,
                    self.profile.name,
                    "probability",
                    self.bank_revision,
                    decision_index,
                    body.probability.denominator,
                )
                >= body.probability.numerator
            ):
                return
        ordered = sorted(active, key=self._selection_key)
        if isinstance(selection := self.selection, IndexPattern):
            index = selection.indices[self.pattern_position]
            self.pattern_position = (self.pattern_position + 1) % len(selection.indices)
            if selection.boundary == "rest" and index >= len(ordered):
                return
            note = ordered[index % len(ordered)]
        elif isinstance(selection, Choice):
            candidates = [
                (
                    n,
                    selection.weights[i % len(selection.weights)]
                    if selection.extend == "repeat" or i < len(selection.weights)
                    else 1,
                )
                for i, n in enumerate(ordered)
                if not selection.no_repeat
                or len(ordered) == 1
                or self.previous_note is None
                or n.id != self.previous_note.id
            ]
            assert body.seed is not None
            chosen = draw_below(
                body.seed,
                self.profile.name,
                "choice",
                self.bank_revision,
                self.choice_count,
                sum(w for _, w in candidates),
            )
            self.choice_count += 1
            index = 0
            while chosen >= candidates[index][1]:
                chosen -= candidates[index][1]
                index += 1
            note = candidates[index][0]
        elif isinstance(selection, Shuffle):
            note = self._shuffle_note(ordered, selection)
        elif isinstance(selection, (InsideOut, OutsideIn)):
            size = len(ordered)
            indices = sorted(
                range(size),
                key=lambda i: (
                    abs(2 * i - (size - 1))
                    if isinstance(selection, InsideOut)
                    else min(i, size - 1 - i)
                ),
            )
            ordered = [ordered[i] for i in indices]
            previous = next(
                (
                    i
                    for i, n in enumerate(ordered)
                    if self.previous_note is not None and n.id == self.previous_note.id
                ),
                None,
            )
            note = ordered[0 if previous is None else (previous + 1) % size]
        elif isinstance(selection, Alternating):
            if self.previous_note is None:
                self.rising = True
                note = ordered[0]
            elif len(ordered) == 1:
                note = ordered[0]
            else:
                previous_key = self._selection_key(self.previous_note)
                candidates = ordered if self.rising else list(reversed(ordered))
                note = next(
                    (
                        n
                        for n in candidates
                        if (
                            self._selection_key(n) > previous_key
                            if self.rising
                            else self._selection_key(n) < previous_key
                        )
                    ),
                    None,
                )
                if note is None:
                    self.rising = not self.rising
                    candidates.reverse()
                    note = next(
                        (
                            n
                            for n in candidates
                            if (
                                self._selection_key(n) >= previous_key
                                if self.rising
                                else self._selection_key(n) <= previous_key
                            )
                            and (
                                selection.repeat_endpoints
                                or n.id != self.previous_note.id
                            )
                        ),
                        candidates[0],
                    )
        elif isinstance(selection, Walk):
            previous = next(
                (
                    i
                    for i, n in enumerate(ordered)
                    if self.previous_note is not None and n.id == self.previous_note.id
                ),
                None,
            )
            move = previous is not None or (
                selection.start == "move"
                if self.previous_note is None
                else selection.on_remove == "rank"
            )
            rank = (
                previous
                if previous is not None
                else (
                    self.walk_rank % len(ordered)
                    if self.previous_note is not None and selection.on_remove == "rank"
                    else 0
                )
            )
            if move:
                chosen = draw_below(
                    self.profile.body.seed or 0,
                    self.profile.name,
                    "walk",
                    self.bank_revision,
                    self.walk_count,
                    sum(selection.weights),
                )
                for offset, weight in zip(
                    selection.moves, selection.weights, strict=True
                ):
                    if chosen < weight:
                        rank = (rank + offset) % len(ordered)
                        break
                    chosen -= weight
            note = ordered[rank]
            self.walk_rank = rank
            self.walk_count += 1
        else:
            note = next(
                (
                    n
                    for n in ordered
                    if self.previous_note is None
                    or self._selection_key(n) > self._selection_key(self.previous_note)
                ),
                ordered[0],
            )
        self.previous_note = note
        interval = decision.duration / decision.repeats
        self.pending.extend(
            _Attack(
                at=at + i * interval,
                note=note,
                gate=decision.final_gate
                if i == decision.repeats - 1
                else decision.gate,
            )
            for i in range(decision.repeats)
        )

    def _shuffle_note(
        self, ordered: list[_InputNote], selection: Shuffle
    ) -> _InputNote:
        seed = self.profile.body.seed
        assert seed is not None
        identities = [n.id for n in ordered]
        edited = self.shuffle_revision != self.bank_revision
        if edited and self.shuffle_order and selection.on_edit == "preserve":
            played = self.shuffle_order[: self.shuffle_position]
            self.shuffle_order = [i for i in self.shuffle_order if i in identities]
            self.shuffle_position = sum(i in identities for i in played)
            for identity in identities:
                if identity not in self.shuffle_order:
                    offset = draw_below(
                        seed,
                        self.profile.name,
                        "shuffle",
                        self.bank_revision,
                        self.shuffle_count,
                        len(self.shuffle_order) - self.shuffle_position + 1,
                    )
                    self.shuffle_count += 1
                    self.shuffle_order.insert(self.shuffle_position + offset, identity)
        restart = not self.shuffle_order or (edited and selection.on_edit == "restart")
        finished = self.shuffle_position == len(self.shuffle_order)
        if restart or (finished and selection.mode == "cycle"):
            self.shuffle_order = identities
            self.shuffle_position = 0
            for index in range(len(self.shuffle_order) - 1, 0, -1):
                chosen = draw_below(
                    seed,
                    self.profile.name,
                    "shuffle",
                    self.bank_revision,
                    self.shuffle_count,
                    index + 1,
                )
                self.shuffle_count += 1
                self.shuffle_order[index], self.shuffle_order[chosen] = (
                    self.shuffle_order[chosen],
                    self.shuffle_order[index],
                )
            if (
                selection.no_repeat
                and len(self.shuffle_order) > 1
                and self.previous_note is not None
                and self.shuffle_order[0] == self.previous_note.id
            ):
                chosen = 1 + draw_below(
                    seed,
                    self.profile.name,
                    "shuffle",
                    self.bank_revision,
                    self.shuffle_count,
                    len(self.shuffle_order) - 1,
                )
                self.shuffle_count += 1
                self.shuffle_order[0], self.shuffle_order[chosen] = (
                    self.shuffle_order[chosen],
                    self.shuffle_order[0],
                )
        elif finished:
            self.shuffle_position = 0
        self.shuffle_revision = self.bank_revision
        identity = self.shuffle_order[self.shuffle_position]
        self.shuffle_position += 1
        return next(n for n in ordered if n.id == identity)

    def _attack(self, attack: _Attack) -> list[LiveEvent]:
        at, note = attack.at, attack.note
        events: list[LiveEvent] = []
        remaining: list[_SoundingNote] = []
        for sounding in self.sounding:
            if sounding.key == note.key:
                events.append(
                    LiveEvent(
                        at=at,
                        kind="off",
                        id=sounding.id,
                        source_id=sounding.source_id,
                        key=sounding.key,
                        velocity=0,
                    )
                )
            else:
                remaining.append(sounding)
        self.sounding = remaining
        output_id = self.next_output_id
        self.next_output_id += 1
        events.append(
            LiveEvent(
                at=at,
                kind="on",
                id=output_id,
                source_id=note.id,
                key=note.key,
                velocity=note.velocity,
            )
        )
        if (end := at + attack.gate) == at:
            events.append(
                LiveEvent(
                    at=at,
                    kind="off",
                    id=output_id,
                    source_id=note.id,
                    key=note.key,
                    velocity=0,
                )
            )
        else:
            self.sounding.append(
                _SoundingNote(id=output_id, source_id=note.id, key=note.key, end=end)
            )
        return events

    def _release_all(self, at: Fraction) -> list[LiveEvent]:
        events = [
            LiveEvent(
                at=at, kind="off", id=n.id, source_id=n.source_id, key=n.key, velocity=0
            )
            for n in self.sounding
        ]
        self.sounding.clear()
        self.pending.clear()
        return events

    def _selection_key(self, note: _InputNote) -> tuple[Fraction, int]:
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
    source_id: int
    key: int
    end: Fraction


class _Attack(BaseModel, frozen=True):
    at: Fraction
    note: _InputNote
    gate: Fraction
