"""Incremental note arpeggiation without MIDI or audio devices.

Feed note changes in beat order, then call ``advance`` to play through a beat.
Changes at the same beat are applied before that beat's arpeggio step.
"""

from __future__ import annotations

from fractions import Fraction
from typing import Literal, Self

from pydantic import BaseModel, Field, model_validator
from ufor import arpeggiator_ports
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
    channel: int = Field(default=1, ge=1, le=16)
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
    lanes: dict[str, LiveArpeggiator] = Field(default_factory=dict)

    @model_validator(mode="after")
    def prepare_ports(self) -> Self:
        body = self.profile.body
        if (
            body.expression.source != "current"
            or body.expression.timing != "original"
            or body.expression.gaps != "omit"
            or "recorded" in body.expression.lanes.values()
        ):
            raise ValueError(
                "held and latched playback require current, original expression"
            )
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
            strum=body.strum,
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
        if body.lanes is not None and not self.lanes:
            self.lanes = {
                n: LiveArpeggiator(
                    profile=ArpeggiatorScore(
                        name=f"{self.profile.name}.{n}", title=n, body=b
                    )
                )
                for n, b in sorted(body.lanes.items())
            }
        return self

    def note_on(self, at: Fraction, key: int, velocity: int) -> list[LiveEvent]:
        value = MusicalValue(key=key, velocity=velocity)
        events = self.press(at, key, key, value)
        events.extend(
            e for a in self.lanes.values() for e in a.note_on(at, key, velocity)
        )
        return sorted(events, key=lambda e: e.at)

    def note_off(self, at: Fraction, key: int) -> list[LiveEvent]:
        events = self.release(at, key)
        events.extend(e for a in self.lanes.values() for e in a.note_off(at, key))
        return sorted(events, key=lambda e: e.at)

    def advance(self, through: Fraction) -> list[LiveEvent]:
        events = super().advance(through)
        events.extend(e for a in self.lanes.values() for e in a.advance(through))
        return sorted(events, key=lambda e: e.at)

    def next_deadline(self) -> Fraction:
        return min(
            [super().next_deadline(), *(a.next_deadline() for a in self.lanes.values())]
        )

    def before(self, through: Fraction) -> list[LiveEvent]:
        events = super().before(through)
        events.extend(e for a in self.lanes.values() for e in a.before(through))
        return sorted(events, key=lambda e: e.at)

    def pause(self, at: Fraction) -> list[LiveEvent]:
        events = super().pause(at)
        events.extend(e for a in self.lanes.values() for e in a.pause(at))
        return sorted(events, key=lambda e: e.at)

    def relocate(self, at: Fraction) -> list[LiveEvent]:
        events = super().relocate(at)
        events.extend(e for a in self.lanes.values() for e in a.relocate(at))
        return sorted(events, key=lambda e: e.at)

    def stop(self, at: Fraction) -> list[LiveEvent]:
        events = super().stop(at)
        events.extend(e for a in self.lanes.values() for e in a.stop(at))
        return sorted(events, key=lambda e: e.at)

    def clear(self, at: Fraction) -> list[LiveEvent]:
        if self.lanes:
            if not any(
                isinstance(a.profile.body.bank, LatchedBank)
                for a in [self, *self.lanes.values()]
            ):
                raise ValueError("clear requires a latched bank")
            events = (
                super().clear(at)
                if isinstance(self.profile.body.bank, LatchedBank)
                else super().before(at)
            )
        else:
            events = super().clear(at)
        events.extend(
            e
            for a in self.lanes.values()
            for e in (
                a.clear(at)
                if isinstance(a.profile.body.bank, LatchedBank)
                else a.before(at)
            )
        )
        return sorted(events, key=lambda e: e.at)

    def control(
        self, at: Fraction, control: arpeggiator_ports.ArpeggiatorControl
    ) -> list[LiveEvent]:
        for lane in [self, *self.lanes.values()]:
            assert lane.ports is not None
            lane.ports.check_control(control, lane.settings.seed)
        events = super().control(at, control)
        events.extend(e for a in self.lanes.values() for e in a.control(at, control))
        return sorted(events, key=lambda e: e.at)

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
        self, note: Item[MusicalValue], accent: Fraction, pitch_interval: int
    ) -> MusicalValue | None:
        assert self.ports is not None
        period = self.ports.tuning.period if self.ports.tuning is not None else 12
        pitch = (
            note.rank
            + (period or 12) * self.profile.body.octaves.offsets[note.layer]
            + pitch_interval
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
            channel=self.profile.body.channel,
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
