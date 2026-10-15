"""Step-published performance values and bounded Envelfo output events."""

from bisect import bisect_right
from fractions import Fraction
from math import ceil, floor, isfinite

from ufor import arpeggiator_ports
from ufor.arpeggiator import (
    PitchRegister,
    ScaleTransform,
    Transposition,
)

from .timing_ports import TimingPorts
from .tuning import PreparedTuning, RealizedPitch


class PerformancePorts(TimingPorts):
    transposition: Transposition = Transposition()
    register_limits: PitchRegister | None = None
    scale: ScaleTransform | None = None
    tuning: PreparedTuning | None = None

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
        if (
            value := self.pending.pop(
                arpeggiator_ports.ArpeggiatorInputPort.transposition, None
            )
        ) is not None:
            self.transposition = self.transposition.model_copy(
                update={"semitones": int(value)}
            )
        return super().begin_step(at, index, revision, capture_ready, cycle)

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
