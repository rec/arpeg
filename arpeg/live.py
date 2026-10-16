"""Incremental note arpeggiation without MIDI or audio devices.

Feed note changes in beat order, then call ``advance`` to play through a beat.
Changes at the same beat are applied before that beat's arpeggio step.
"""

from __future__ import annotations

from fractions import Fraction
from typing import Literal, Self

from pydantic import BaseModel, Field, model_validator
from ufor.arpeggiator import (
    Alternating,
    ArpeggiatorScore,
    Ascending,
    Choice,
    Descending,
    Euclidean,
    Grammar,
    Grid,
    HeldBank,
    IndexPattern,
    InsideOut,
    IntervalEnvelope,
    LatchedBank,
    OctaveTraversal,
    OutsideIn,
    Pattern,
    Played,
    Shuffle,
    StepPatterns,
    Transition,
    Walk,
)

from .engine import Arpeggiator, EngineSettings, Item
from .expression import accent_velocity
from .inversion import inversion_offsets
from .octaves import expanded_indices
from .ports import PerformancePorts
from .tuning import PreparedTuning


class LiveEvent(BaseModel, frozen=True):
    at: Fraction
    kind: Literal["on", "off"]
    id: int
    source_id: int
    key: int = Field(ge=0, le=127)
    tuning_semitones: float = 0
    velocity: int = Field(ge=0, le=127)


class MusicalValue(BaseModel, frozen=True):
    key: int = Field(ge=0, le=127)
    velocity: int = Field(ge=1, le=127)
    tuning_semitones: float = 0


class LiveArpeggiator(Arpeggiator[MusicalValue, LiveEvent]):
    """Musical preparation and MIDI realization around the shared item engine."""

    profile: ArpeggiatorScore = Field(frozen=True)
    ports: PerformancePorts | None = None

    @model_validator(mode="after")
    def prepare_ports(self) -> Self:
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
                Grammar,
                Shuffle,
                Choice,
                Transition,
                IntervalEnvelope,
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
        self.settings = EngineSettings(
            name=self.profile.name,
            bank=body.bank,
            selection=body.selection,
            rhythm=body.rhythm,
            gate=body.gate,
            patterns=body.patterns or StepPatterns(),
            probability=body.probability,
            seed=body.seed,
            retrigger=body.retrigger,
        )
        if self.ports is None:
            self.ports = PerformancePorts(
                gate=body.gate,
                density=body.probability,
                transposition=body.transposition,
                selection_offset=body.selection_offset,
                register_limits=body.register_limits,
                scale=body.scale,
                tuning=PreparedTuning.prepare(body),
            )
        return self

    def note_on(self, at: Fraction, key: int, velocity: int) -> list[LiveEvent]:
        value = MusicalValue(key=key, velocity=velocity)
        return self.press(at, key, key, value)

    def note_off(self, at: Fraction, key: int) -> list[LiveEvent]:
        return self.release(at, key)

    def _expand(self, active: list[Item[MusicalValue]]) -> list[Item[MusicalValue]]:
        assert self.ports is not None
        period = self.ports.tuning.period if self.ports.tuning is not None else 12
        period = period or 12
        offsets = inversion_offsets(
            [n.value.key for n in active], self.profile.body.inversion, period
        )
        voiced = [0] * len(active)
        if (voicing := self.profile.body.voicing) is not None:
            ranked = sorted(
                range(len(active)),
                key=lambda i: (active[i].value.key + offsets[i], active[i].id),
            )
            for i, j in enumerate(ranked[: len(voicing)]):
                voiced[j] = period * voicing[i]
        octaves = self.profile.body.octaves
        active = [
            active[i].model_copy(
                update={
                    "id": active[i].id * len(octaves.offsets) + j,
                    "source_id": active[i].id,
                    "layer": j,
                    "rank": active[i].value.key + offsets[i] + voiced[i],
                }
            )
            for i, j in expanded_indices(len(active), octaves)
        ]
        return active

    def _realize(
        self, note: Item[MusicalValue], accent: Fraction
    ) -> MusicalValue | None:
        assert self.ports is not None
        period = self.ports.tuning.period if self.ports.tuning is not None else 12
        pitch = (
            note.rank + (period or 12) * self.profile.body.octaves.offsets[note.layer]
        )
        if (realized := self.ports.realize_pitch(pitch)) is None:
            return None
        return MusicalValue(
            key=realized.key,
            velocity=accent_velocity(note.value.velocity, accent),
            tuning_semitones=realized.tuning_semitones,
        )

    def _conflicts(self, left: MusicalValue, right: MusicalValue) -> bool:
        return left.key == right.key

    def _event(
        self,
        at: Fraction,
        kind: Literal["on", "off"],
        id: int,
        source_id: int,
        value: MusicalValue,
        accent: Fraction,
    ) -> LiveEvent:
        return LiveEvent(
            at=at,
            kind=kind,
            id=id,
            source_id=source_id,
            key=value.key,
            velocity=value.velocity if kind == "on" else 0,
            tuning_semitones=value.tuning_semitones if kind == "on" else 0,
        )

    def _entry_key(
        self, note: Item[MusicalValue], position: Fraction, reverse: bool
    ) -> tuple[int, Fraction, int, int]:
        layer = -note.layer if reverse else note.layer
        if self.profile.body.octaves.traversal == OctaveTraversal.notes_first:
            return layer, position, note.source_id, 0
        return 0, position, note.source_id, layer
