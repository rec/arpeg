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
        repeats = step.repeats if isinstance(step, HitStep) else 0
        if isinstance(step, HitStep) and (condition := step.condition) is not None:
            current_pass = index // len(rhythm.steps) % condition.out_of + 1
            if current_pass not in condition.passes:
                repeats = 0
        attack_gate = duration / max(1, repeats) * gate
        final_gate = attack_gate
        if repeats:
            held = duration / repeats
            for offset in range(1, len(rhythm.steps)):
                tied = rhythm.steps[(index + offset) % len(rhythm.steps)]
                if not isinstance(tied, TieStep):
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
            Fraction(rhythm.step.removesuffix(" beat")), rhythm.swing, index
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
    if isinstance(rhythm, Euclidean):
        return index % rhythm.steps == 0
    if isinstance(rhythm, Pattern):
        return index % len(rhythm.steps) == 0
    return False


def allows_step(rhythm: Grid | Euclidean, index: int) -> bool:
    """Positive rotation moves hits later; zero pulses produces silence."""
    if isinstance(rhythm, Grid):
        return True
    phase = (index - rhythm.rotation) % rhythm.steps
    return (phase * rhythm.pulses) % rhythm.steps < rhythm.pulses


def grid_duration(step: Fraction, swing: Fraction, index: int) -> Fraction:
    """Stretch the first interval and shorten the second, preserving pair duration."""
    return 2 * step * (swing if index % 2 == 0 else 1 - swing)


def grid_onset(step: Fraction, swing: Fraction, index: int) -> Fraction:
    pairs, second = divmod(index, 2)
    return 2 * step * (pairs + (swing if second else 0))


def grid_position(
    step: Fraction, swing: Fraction, at: Fraction
) -> tuple[int, Fraction]:
    """Find the first opportunity at or after a beat, retaining absolute pair phase."""
    pair = 2 * step
    pairs, offset = divmod(at, pair)
    if offset == 0:
        return 2 * pairs, at
    middle = pair * (pairs + swing)
    if at <= middle:
        return 2 * pairs + 1, middle
    return 2 * pairs + 2, pair * (pairs + 1)
