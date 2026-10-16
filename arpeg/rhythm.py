"""Canonical rhythm masks on the arpeggiator's local step clock."""

from fractions import Fraction

from pydantic import BaseModel
from ufor.arpeggiator import Euclidean, Grid, HitStep, Pattern, StepPatterns, TieStep


class RhythmDecision(BaseModel, frozen=True):
    duration: Fraction
    repeats: int
    gate: Fraction
    final_gate: Fraction
    accent: Fraction


def decide_step(
    rhythm: Grid | Euclidean | Pattern,
    index: int,
    gate: Fraction,
    patterns: StepPatterns,
) -> RhythmDecision:
    """Resolve one opportunity, including the final attack's tied gate."""
    base_gate = gate
    gate *= patterns.gates[index % len(patterns.gates)]
    if isinstance(rhythm, Pattern):
        step = rhythm.steps[index % len(rhythm.steps)]
        duration = Fraction(step.duration.removesuffix(" beat"))
        repeats = (
            step.repeats
            if isinstance(step, HitStep) and allows_step(rhythm, index)
            else 0
        )
        attack_gate = duration / max(1, repeats) * gate
        final_gate = attack_gate
        if repeats:
            held = duration / repeats
            for offset in range(1, len(rhythm.steps)):
                tied = rhythm.steps[(index + offset) % len(rhythm.steps)]
                if not within_cycles(rhythm, index + offset) or not isinstance(
                    tied, TieStep
                ):
                    break
                tied_duration = Fraction(tied.duration.removesuffix(" beat"))
                final_gate = max(
                    final_gate,
                    held
                    + tied_duration
                    * base_gate
                    * patterns.gates[(index + offset) % len(patterns.gates)],
                )
                held += tied_duration
    else:
        duration = grid_duration(
            Fraction(rhythm.step.removesuffix(" beat")),
            rhythm.swing,
            index,
            rhythm.groove,
        )
        repeats = int(allows_step(rhythm, index))
        attack_gate = final_gate = duration * gate
    return RhythmDecision(
        duration=duration,
        repeats=repeats,
        gate=attack_gate,
        final_gate=final_gate,
        accent=patterns.accents[index % len(patterns.accents)],
    )


def starts_cycle(rhythm: Grid | Euclidean | Pattern, index: int) -> bool:
    """An authored rhythm loop starts at cell zero, independently of its hits."""
    if not within_cycles(rhythm, index):
        return False
    if isinstance(rhythm, Euclidean):
        return index % rhythm.steps == 0
    if isinstance(rhythm, Pattern):
        return index % len(rhythm.steps) == 0
    return False


def within_cycles(rhythm: Grid | Euclidean | Pattern, index: int) -> bool:
    """Finite loops consume their allowance on the absolute rhythm clock."""
    if isinstance(rhythm, Grid) or rhythm.cycles is None:
        return True
    length = rhythm.steps if isinstance(rhythm, Euclidean) else len(rhythm.steps)
    return index // length < rhythm.cycles


def allows_step(rhythm: Grid | Euclidean | Pattern, index: int) -> bool:
    """Apply finite limits and the rhythm's mask or per-hit condition."""
    if not within_cycles(rhythm, index):
        return False
    if isinstance(rhythm, Grid):
        return True
    if isinstance(rhythm, Pattern):
        step = rhythm.steps[index % len(rhythm.steps)]
        if not isinstance(step, HitStep):
            return False
        if (condition := step.condition) is None:
            return True
        current_pass = index // len(rhythm.steps) % condition.out_of + 1
        return current_pass in condition.passes
    phase = (index - rhythm.rotation) % rhythm.steps
    return (phase * rhythm.pulses) % rhythm.steps < rhythm.pulses


def grid_duration(
    step: Fraction, swing: Fraction, index: int, groove: list[Fraction] | None = None
) -> Fraction:
    """Shape intervals while preserving each timing template's total duration."""
    if groove is not None:
        return (
            step * len(groove) * groove[index % len(groove)] / sum(groove, Fraction(0))
        )
    return 2 * step * (swing if index % 2 == 0 else 1 - swing)


def grid_onset(
    step: Fraction, swing: Fraction, index: int, groove: list[Fraction] | None = None
) -> Fraction:
    if groove is not None:
        cycles, position = divmod(index, len(groove))
        period = step * len(groove)
        return period * (
            cycles + sum(groove[:position], Fraction(0)) / sum(groove, Fraction(0))
        )
    pairs, second = divmod(index, 2)
    return 2 * step * (pairs + (swing if second else 0))


def grid_position(
    step: Fraction, swing: Fraction, at: Fraction, groove: list[Fraction] | None = None
) -> tuple[int, Fraction]:
    """Find the first opportunity at or after a beat, retaining template phase."""
    if groove is not None:
        period = step * len(groove)
        cycles = at // period
        onset = cycles * period
        scale = period / sum(groove, Fraction(0))
        for position, weight in enumerate(groove):
            if onset >= at:
                return cycles * len(groove) + position, onset
            onset += weight * scale
        return (cycles + 1) * len(groove), onset
    pair = 2 * step
    pairs, offset = divmod(at, pair)
    if offset == 0:
        return 2 * pairs, at
    middle = pair * (pairs + swing)
    if at <= middle:
        return 2 * pairs + 1, middle
    return 2 * pairs + 2, pair * (pairs + 1)
