"""Prepare shared uFor tuning definitions as unrounded MIDI pitch coordinates."""

from fractions import Fraction
from math import exp2, isfinite, log2
from typing import Self

from pydantic import BaseModel, Field
from ufor.arpeggiator import Arpeggiator
from ufor.expression import MAX_FRACTION_BITS, evaluate
from ufor.tuning import Computed, FrequencyTable, IntervalPattern, RatioTable


class PreparedTuning(BaseModel, frozen=True):
    computed_limit: int | None = None
    computed_ratio: Fraction | float = Fraction(1)
    pitches: list[float]
    first_note: int
    period: int | None
    period_semitones: float = 0

    @classmethod
    def prepare(cls, body: Arpeggiator) -> Self | None:
        if (tuning := body.tuning) is None:
            return None
        source = tuning.source
        first = tuning.root_note
        period = None
        span = 0.0
        if isinstance(source, FrequencyTable):
            first = source.first_note
            pitches = [
                69 + 12 * (pitch_logarithm(v) - log2(440)) for v in source.values
            ]
        else:
            root = 69 + 12 * (pitch_logarithm(tuning.root_frequency) - log2(440))
            if isinstance(source, Computed):
                period = source.notes_per_octave
                span = 12 * pitch_logarithm(source.octave_ratio)
                pitches = [root]
            elif isinstance(source, RatioTable):
                pitches = [root + 12 * pitch_logarithm(v) for v in source.values]
                if source.repeat_ratio is not None:
                    period = len(pitches)
                    span = 12 * pitch_logarithm(source.repeat_ratio)
            else:
                assert isinstance(source, IntervalPattern)
                pitches = [root]
                for v in source.intervals:
                    pitches.append(pitches[-1] + 12 * pitch_logarithm(v))
                if source.repeat:
                    period = len(source.intervals)
                    span = pitches.pop() - root
            pitches = [p + tuning.detune_cents / 100 for p in pitches]
        if isinstance(source, FrequencyTable):
            pitches = [p + tuning.detune_cents / 100 for p in pitches]
        return cls(
            pitches=pitches,
            first_note=first,
            period=period,
            period_semitones=span,
            computed_limit=source.denominator_limit
            if isinstance(source, Computed)
            else None,
            computed_ratio=evaluate(source.octave_ratio)
            if isinstance(source, Computed)
            else Fraction(1),
        )

    def pitch(self, index: int) -> float:
        index -= self.first_note
        if self.computed_limit is not None:
            if not self.computed_limit:
                assert self.period is not None
                return self.pitches[0] + index * self.period_semitones / self.period
            try:
                assert self.period is not None
                exponent = Fraction(index, self.period)
                estimate = exp2(index * self.period_semitones / (12 * self.period))
                if not isfinite(estimate) or estimate <= 0:
                    raise ValueError("computed ratio is outside numerical limits")
                if (
                    isinstance(self.computed_ratio, Fraction)
                    and exponent.denominator == 1
                    and max(
                        self.computed_ratio.numerator.bit_length(),
                        self.computed_ratio.denominator.bit_length(),
                    )
                    * abs(exponent.numerator)
                    > MAX_FRACTION_BITS
                ):
                    raise ValueError("computed ratio exceeds exact-number limit")
                ratio = Fraction(self.computed_ratio**exponent).limit_denominator(
                    self.computed_limit
                )
                logarithm = (
                    log2(ratio.numerator) - log2(ratio.denominator)
                    if isinstance(ratio, Fraction)
                    else log2(ratio)
                )
            except (ValueError, OverflowError) as error:
                raise RuntimeError(
                    "computed tuning cannot realize this note"
                ) from error
            return self.pitches[0] + 12 * logarithm
        if self.period is None:
            if not 0 <= index < len(self.pitches):
                raise RuntimeError("note is outside the finite tuning")
            return self.pitches[index]
        cycle, rank = divmod(index, self.period)
        return self.pitches[rank] + cycle * self.period_semitones


class RealizedPitch(BaseModel, frozen=True):
    key: int = Field(ge=0, le=127)
    tuning_semitones: float = 0


def pitch_logarithm(expression: str) -> float:
    value = evaluate(expression)
    return (
        log2(value.numerator) - log2(value.denominator)
        if isinstance(value, Fraction)
        else log2(value)
    )
