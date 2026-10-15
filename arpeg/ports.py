"""Step-published performance values and bounded Envelfo output events."""

from bisect import bisect_right
from fractions import Fraction
from math import ceil, floor, isfinite

from pydantic import BaseModel, Field
from ufor import arpeggiator_ports
from ufor.arpeggiator import (
    PitchRegister,
    ScaleTransform,
    SelectionOffset,
    Transposition,
)

from .tuning import PreparedTuning, RealizedPitch


class PerformancePorts(BaseModel):
    gate: Fraction
    density: Fraction
    transposition: Transposition = Transposition()
    selection_offset: SelectionOffset = SelectionOffset()
    register_limits: PitchRegister | None = None
    scale: ScaleTransform | None = None
    tuning: PreparedTuning | None = None
    pending: dict[arpeggiator_ports.ArpeggiatorInputPort, Fraction] = Field(
        default_factory=dict
    )
    events: list[arpeggiator_ports.ArpeggiatorOutput] = Field(
        default_factory=list, max_length=4096
    )
    exhausted: bool = False

    def check_control(
        self, control: arpeggiator_ports.ArpeggiatorControl, seed: int | None
    ) -> None:
        if control.port in (
            arpeggiator_ports.ArpeggiatorInputPort.breath,
            arpeggiator_ports.ArpeggiatorInputPort.bend,
            arpeggiator_ports.ArpeggiatorInputPort.pressure,
        ):
            raise ValueError("expression controls require the MIDI player")
        if (
            control.port == arpeggiator_ports.ArpeggiatorInputPort.density
            and 0 < control.value < 1
            and seed is None
        ):
            raise ValueError("density between zero and one requires an explicit seed")

    def begin_step(
        self,
        at: Fraction,
        index: int,
        revision: int,
        capture_ready: bool = False,
        cycle: bool = False,
    ) -> bool:
        for port, value in self.pending.items():
            if port == arpeggiator_ports.ArpeggiatorInputPort.gate:
                self.gate = value
            elif port == arpeggiator_ports.ArpeggiatorInputPort.density:
                self.density = value
            elif port == arpeggiator_ports.ArpeggiatorInputPort.transposition:
                self.transposition = self.transposition.model_copy(
                    update={"semitones": int(value)}
                )
            else:
                self.selection_offset = self.selection_offset.model_copy(
                    update={"ranks": int(value)}
                )
        self.pending.clear()
        if len(self.events) > 4094 - int(capture_ready) - int(cycle):
            self.exhausted = True
            return False
        if capture_ready:
            self.events.append(
                arpeggiator_ports.ArpeggiatorOutput(
                    at=at,
                    port=arpeggiator_ports.ArpeggiatorOutputPort.capture_ready,
                    index=index,
                    revision=revision,
                )
            )
        if cycle:
            self.events.append(
                arpeggiator_ports.ArpeggiatorOutput(
                    at=at,
                    port=arpeggiator_ports.ArpeggiatorOutputPort.cycle,
                    index=index,
                    revision=revision,
                )
            )
        self.events.append(
            arpeggiator_ports.ArpeggiatorOutput(
                at=at,
                port=arpeggiator_ports.ArpeggiatorOutputPort.step,
                index=index,
                revision=revision,
            )
        )
        return True

    def offset_rank(self, rank: int, size: int) -> int | None:
        shifted = rank + self.selection_offset.ranks
        if self.selection_offset.boundary == "rest" and not 0 <= shifted < size:
            return None
        return shifted % size

    def realize_pitch(self, key: int) -> RealizedPitch | None:
        if (scale := self.scale) is not None:
            definition = scale.definition
            intervals = definition.note_numbers
            period = definition.octave_length
            octave, chroma = divmod(key - definition.offset, period)
            size = len(intervals)
            rank = bisect_right(intervals, chroma) - 1
            lower = intervals[rank] if rank >= 0 else intervals[-1] - period
            if lower != chroma:
                if scale.mapping == "drop":
                    return None
                upper = (
                    intervals[rank + 1] if rank + 1 < size else intervals[0] + period
                )
                if scale.mapping == "up" or (
                    scale.mapping == "nearest" and upper - chroma < chroma - lower
                ):
                    rank += 1
            octave, rank = divmod(octave * size + rank + scale.degrees, size)
            key = definition.offset + octave * period + intervals[rank]
        pitch = (
            self.tuning.pitch(key) if self.tuning is not None else key
        ) + self.transposition.semitones
        if not isfinite(pitch):
            raise RuntimeError("tuning produced a nonfinite pitch")
        limits = self.register_limits
        low, high = (limits.low, limits.high) if limits is not None else (0, 127)
        boundary = (
            limits.boundary if limits is not None else self.transposition.boundary
        )
        if low <= pitch <= high:
            base = min(127, floor(pitch + 0.5))
            return RealizedPitch(key=base, tuning_semitones=pitch - base)
        match boundary:
            case "drop":
                return None
            case "fold":
                if self.tuning is not None:
                    period = abs(self.tuning.period_semitones)
                    if not period:
                        return None
                    folded = (
                        pitch + ceil((low - pitch) / period) * period
                        if pitch < low
                        else pitch - ceil((pitch - high) / period) * period
                    )
                    if not low <= folded <= high:
                        return None
                    base = min(127, floor(folded + 0.5))
                    return RealizedPitch(key=base, tuning_semitones=folded - base)
                folded = (
                    low + (pitch - low) % 12
                    if pitch < low
                    else high - (high - pitch) % 12
                )
                return RealizedPitch(key=folded) if low <= folded <= high else None
            case "error":
                if self.tuning is not None:
                    raise RuntimeError("tuned pitch is outside configured MIDI limits")
                if limits is not None:
                    raise RuntimeError("pitch is outside configured register limits")
                raise ValueError("transposed pitch is outside MIDI range 0–127")

    def outcome(self, hit: bool) -> None:
        step = self.events[-1]
        self.events.append(
            arpeggiator_ports.ArpeggiatorOutput(
                at=step.at,
                port=arpeggiator_ports.ArpeggiatorOutputPort.hit
                if hit
                else arpeggiator_ports.ArpeggiatorOutputPort.rest,
                index=step.index,
                revision=step.revision,
            )
        )

    def take_events(self) -> arpeggiator_ports.ArpeggiatorPortBatch:
        batch = arpeggiator_ports.ArpeggiatorPortBatch(
            events=self.events, exhausted=self.exhausted
        )
        self.events = []
        self.exhausted = False
        return batch
