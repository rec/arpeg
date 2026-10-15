"""Payload-independent incremental arpeggiation in exact beats."""

from __future__ import annotations

from abc import abstractmethod
from fractions import Fraction
from functools import cached_property
from math import ceil
from typing import Generic, Literal, Self, TypeVar

from pydantic import BaseModel, Field, SerializeAsAny, model_validator
from ufor import arpeggiator_ports
from ufor.arpeggiator import (
    Alternating,
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
    TieStep,
    Walk,
)

from .chance import draw_below
from .rhythm import RhythmDecision, decide_step, starts_cycle
from .timing_ports import TimingPorts

Payload = TypeVar("Payload")
Event = TypeVar("Event")


class EngineSettings(BaseModel, frozen=True):
    name: str = "items"
    bank: HeldBank | LatchedBank = HeldBank()
    selection: (
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
    ) = Ascending()
    rhythm: Grid | Euclidean | Pattern = Grid(step="1/4 beat")
    gate: Fraction = Fraction(4, 5)
    probability: Fraction = Fraction(1)
    seed: int | None = None
    retrigger: Literal["on_empty", "bank_edit"] = "on_empty"

    @model_validator(mode="after")
    def valid_settings(self) -> Self:
        if self.gate < 0 or not 0 <= self.probability <= 1:
            raise ValueError(
                "gate must be nonnegative and probability must be between zero and one"
            )
        if self.seed is None and (
            0 < self.probability < 1
            or isinstance(self.selection, (Shuffle, Choice))
            or isinstance(self.selection, Walk)
            and len(self.selection.moves) > 1
        ):
            raise ValueError("random decisions require an explicit seed")
        if isinstance(self.selection, Ascending) and self.selection.repeats != 1:
            raise ValueError("live mode does not support repeated selections")
        return self


class Item(BaseModel, Generic[Payload], frozen=True):
    id: int
    source_id: int
    identity: int
    rank: int
    layer: int = 0
    onset: Fraction
    value: Payload


class Occurrence(BaseModel, Generic[Payload], frozen=True):
    at: Fraction
    kind: Literal["on", "off"]
    id: int
    source_id: int
    value: Payload


class Arpeggiator(BaseModel, Generic[Payload, Event]):
    """Shared held/latched bank, selection, rhythm, and occurrence lifecycle.

    Input identities and ordering ranks are independent of opaque payloads.
    Adapters expand items, realize selected values, and construct output events.
    """

    settings: EngineSettings = EngineSettings()
    ports: TimingPorts | None = None
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
    pending: list[SerializeAsAny[_Attack[Payload]]] = Field(default_factory=list)
    now: Fraction = Fraction(0)
    next_input_id: int = 0
    next_output_id: int = 0
    previous_note: SerializeAsAny[Item[Payload]] | None = None
    input: list[SerializeAsAny[Item[Payload]]] = Field(default_factory=list)
    bank: list[SerializeAsAny[Item[Payload]]] = Field(default_factory=list)
    toggle_at: Fraction | None = None
    toggled_identities: list[int] = Field(default_factory=list)
    toggle_added_identities: list[int] = Field(default_factory=list)
    sounding: list[SerializeAsAny[_Active[Payload]]] = Field(default_factory=list)

    @model_validator(mode="after")
    def prepare_ports(self) -> Self:
        if self.ports is None:
            self.ports = TimingPorts(
                gate=self.settings.gate, density=self.settings.probability
            )
        return self

    @cached_property
    def bank_mode(self) -> HeldBank | LatchedBank:
        bank = self.settings.bank
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
        selection = self.settings.selection
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

    def press(
        self, at: Fraction, identity: int, rank: int, value: Payload
    ) -> list[Event]:
        """Add an item, leaving the step at ``at`` for ``advance``."""
        self._check_time(at)
        note = Item[Payload](
            id=self.next_input_id,
            source_id=self.next_input_id,
            identity=identity,
            rank=rank,
            value=value,
            onset=at,
        )
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
                    self.toggled_identities.clear()
                    self.toggle_added_identities.clear()
                if identity not in self.toggled_identities:
                    edited = True
                    self.toggled_identities.append(identity)
                    if any(n.identity == identity for n in self.bank):
                        self.bank = [n for n in self.bank if n.identity != identity]
                    else:
                        self.bank.append(note)
                        self.toggle_added_identities.append(identity)
                elif identity in self.toggle_added_identities:
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
        if edited and self.settings.retrigger == "bank_edit":
            self.previous_note = None
            self.pattern_position = 0
            self.shuffle_order.clear()
            self.shuffle_position = 0
        self.next_input_id += 1
        return events

    def release(self, at: Fraction, identity: int) -> list[Event]:
        """Release the oldest held item with this input identity."""
        self._check_time(at)
        if not any(n.identity == identity for n in self.input):
            raise ValueError("release has no matching onset")
        events = self._process_until(at, inclusive=False)
        self.input.pop(
            next(i for i, n in enumerate(self.input) if n.identity == identity)
        )
        if isinstance(self.bank_mode, HeldBank):
            self.bank_revision += 1
        if (
            isinstance(self.bank_mode, HeldBank)
            and self.settings.retrigger == "bank_edit"
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

    def advance(self, through: Fraction) -> list[Event]:
        """Emit all scheduled events through this beat, inclusive."""
        self._check_time(through)
        return self._process_until(through, inclusive=True)

    def before(self, through: Fraction) -> list[Event]:
        """Release due notes before expression, leaving this beat's attacks pending."""
        self._check_time(through)
        events = self._process_until(through, inclusive=False)
        events.extend(self._release_due(through))
        return events

    def control(
        self, at: Fraction, control: arpeggiator_ports.ArpeggiatorControl
    ) -> list[Event]:
        self._check_time(at)
        assert self.ports is not None
        self.ports.check_control(control, self.settings.seed)
        events = self.before(at)
        self.ports.pending[control.port] = control.value
        return events

    def stop(self, at: Fraction) -> list[Event]:
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
        self.toggled_identities.clear()
        self.toggle_added_identities.clear()
        self.previous_note = None
        self.pattern_position = 0
        self.shuffle_order.clear()
        self.shuffle_position = 0
        self.now = at
        return events

    def clear(self, at: Fraction) -> list[Event]:
        """Clear a latched bank and release its owned output notes."""
        self._check_time(at)
        if isinstance(self.bank_mode, HeldBank):
            raise ValueError("clear requires a latched bank")
        events = self._process_until(at, inclusive=False)
        if self.bank:
            self.bank_revision += 1
        self.bank.clear()
        self.toggle_at = None
        self.toggled_identities.clear()
        self.toggle_added_identities.clear()
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

    def pause(self, at: Fraction) -> list[Event]:
        """Cancel output while retaining input notes and selector position."""
        events = self._release_all(at)
        self.now = at
        rhythm = self.settings.rhythm
        assert isinstance(rhythm, (Grid, Euclidean, Pattern))
        while self.next_step < at:
            self.next_step += decide_step(
                rhythm, self.step_index, self.settings.gate
            ).duration
            self.step_index += 1
        return events

    def relocate(self, at: Fraction) -> list[Event]:
        """Seek the rhythm and restart traversal without forgetting held notes."""
        events = self._release_all(at)
        assert self.ports is not None
        self.ports.pending.clear()
        rhythm = self.settings.rhythm
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
                    rhythm, self.step_index, self.settings.gate
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

    def _process_until(self, through: Fraction, *, inclusive: bool) -> list[Event]:
        events: list[Event] = []
        while (at := self.next_deadline()) < through or (inclusive and at == through):
            events.extend(self._release_due(at))
            if self.next_step == at:
                rhythm = self.settings.rhythm
                assert isinstance(rhythm, (Grid, Euclidean, Pattern))
                assert self.ports is not None
                admitted = self.ports.begin_step(
                    at,
                    self.step_index,
                    self.bank_revision,
                    cycle=starts_cycle(rhythm, self.step_index),
                )
                decision = decide_step(rhythm, self.step_index, self.ports.gate)
                if admitted:
                    hit = self._schedule_step(at, decision)
                    if not (
                        isinstance(rhythm, Pattern)
                        and isinstance(
                            rhythm.steps[self.step_index % len(rhythm.steps)], TieStep
                        )
                    ):
                        self.ports.outcome(hit)
                self.next_step += decision.duration
                self.step_index += 1
            while self.pending and self.pending[0].at == at:
                events.extend(self._attack(self.pending.pop(0)))
        self.now = through
        return events

    def _release_due(self, at: Fraction) -> list[Event]:
        events = [
            self._event(
                at=n.end,
                kind="off",
                id=n.id,
                source_id=n.source_id,
                value=n.value,
            )
            for n in self.sounding
            if n.end <= at
        ]
        self.sounding = [n for n in self.sounding if n.end > at]
        return events

    def _schedule_step(self, at: Fraction, decision: RhythmDecision) -> bool:
        active = self.input if isinstance(self.bank_mode, HeldBank) else self.bank
        assert self.ports is not None
        active = self._expand(active)
        if not active:
            self.previous_note = None
            self.pattern_position = 0
            self.shuffle_order.clear()
            self.shuffle_position = 0
            return False
        if not decision.repeats:
            return False
        body = self.settings
        assert self.ports is not None
        probability = self.ports.density
        decision_index = self.decision_count
        self.decision_count += 1
        if probability == 0:
            return False
        if probability < 1:
            assert body.seed is not None
            if (
                draw_below(
                    body.seed,
                    self.settings.name,
                    "probability",
                    self.bank_revision,
                    decision_index,
                    probability.denominator,
                )
                >= probability.numerator
            ):
                return False
        ordered = sorted(active, key=self._selection_key)
        if isinstance(selection := self.selection, IndexPattern):
            index = selection.indices[self.pattern_position]
            self.pattern_position = (self.pattern_position + 1) % len(selection.indices)
            if selection.boundary == "rest" and index >= len(ordered):
                return False
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
                self.settings.name,
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
                    self.settings.seed or 0,
                    self.settings.name,
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
        if self.ports.selection_offset.ranks:
            ranked = sorted(
                active,
                key=lambda n: self._entry_key(n, Fraction(n.rank), False),
            )
            if (
                rank := self.ports.offset_rank(ranked.index(note), len(ranked))
            ) is None:
                return False
            note = ranked[rank]
        if (value := self._realize(note)) is None:
            return False
        interval = decision.duration / decision.repeats
        self.pending.extend(
            _Attack[Payload](
                at=at + i * interval,
                note=note,
                value=value,
                gate=decision.final_gate
                if i == decision.repeats - 1
                else decision.gate,
            )
            for i in range(decision.repeats)
        )
        return True

    def _shuffle_note(
        self, ordered: list[Item[Payload]], selection: Shuffle
    ) -> Item[Payload]:
        seed = self.settings.seed
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
                        self.settings.name,
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
                    self.settings.name,
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
                    self.settings.name,
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

    def _attack(self, attack: _Attack[Payload]) -> list[Event]:
        at, note = attack.at, attack.note
        events: list[Event] = []
        remaining: list[_Active[Payload]] = []
        for sounding in self.sounding:
            if self._conflicts(sounding.value, attack.value):
                events.append(
                    self._event(
                        at=at,
                        kind="off",
                        id=sounding.id,
                        source_id=sounding.source_id,
                        value=sounding.value,
                    )
                )
            else:
                remaining.append(sounding)
        self.sounding = remaining
        output_id = self.next_output_id
        self.next_output_id += 1
        events.append(
            self._event(
                at=at,
                kind="on",
                id=output_id,
                source_id=note.source_id,
                value=attack.value,
            )
        )
        if (end := at + attack.gate) == at:
            events.append(
                self._event(
                    at=at,
                    kind="off",
                    id=output_id,
                    source_id=note.source_id,
                    value=attack.value,
                )
            )
        else:
            self.sounding.append(
                _Active[Payload](
                    id=output_id, source_id=note.source_id, value=attack.value, end=end
                )
            )
        return events

    def _release_all(self, at: Fraction) -> list[Event]:
        events = [
            self._event(
                at=at, kind="off", id=n.id, source_id=n.source_id, value=n.value
            )
            for n in self.sounding
        ]
        self.sounding.clear()
        self.pending.clear()
        return events

    def _expand(self, active: list[Item[Payload]]) -> list[Item[Payload]]:
        return active

    def _realize(self, note: Item[Payload]) -> Payload | None:
        return note.value

    def _conflicts(self, left: Payload, right: Payload) -> bool:
        return False

    @abstractmethod
    def _event(
        self,
        at: Fraction,
        kind: Literal["on", "off"],
        id: int,
        source_id: int,
        value: Payload,
    ) -> Event:
        raise NotImplementedError

    def _selection_key(self, note: Item[Payload]) -> tuple[int, Fraction, int, int]:
        selection = self.selection
        if isinstance(selection, Played):
            position = -note.onset if selection.direction == "reverse" else note.onset
        else:
            position = Fraction(
                -note.rank if isinstance(selection, Descending) else note.rank
            )
        reverse = (
            isinstance(selection, Descending)
            or isinstance(selection, Played)
            and selection.direction == "reverse"
        )
        return self._entry_key(note, position, reverse)

    def _entry_key(
        self, note: Item[Payload], position: Fraction, reverse: bool
    ) -> tuple[int, Fraction, int, int]:
        return 0, position, note.source_id, 0


class TokenArpeggiator(Arpeggiator[str, Occurrence[str]]):
    def _event(
        self,
        at: Fraction,
        kind: Literal["on", "off"],
        id: int,
        source_id: int,
        value: str,
    ) -> Occurrence[str]:
        return Occurrence[str](
            at=at, kind=kind, id=id, source_id=source_id, value=value
        )


class _Active(BaseModel, Generic[Payload], frozen=True):
    id: int
    source_id: int
    value: Payload
    end: Fraction


class _Attack(BaseModel, Generic[Payload], frozen=True):
    at: Fraction
    note: SerializeAsAny[Item[Payload]]
    value: Payload
    gate: Fraction
