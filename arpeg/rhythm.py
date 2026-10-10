"""Canonical rhythm masks on the arpeggiator's local step clock."""

from fractions import Fraction

from pydantic import BaseModel
from ufor.arpeggiator import Euclidean, Grid, HitStep, Pattern, TieStep


class RhythmDecision(BaseModel, frozen=True):
    duration: Fraction
    repeats: int
    gate: Fraction
    final_gate: Fraction


def decide_step(
    rhythm: Grid | Euclidean | Pattern, index: int, gate: Fraction
) -> RhythmDecision:
    """Resolve one opportunity, including the final attack's tied gate."""
    if isinstance(rhythm, Pattern):
        step = rhythm.steps[index % len(rhythm.steps)]
        duration = Fraction(step.duration.removesuffix(" beat"))
        repeats = step.repeats if isinstance(step, HitStep) else 0
        attack_gate = duration / max(1, repeats) * gate
        final_gate = attack_gate
        if repeats:
            held = duration / repeats
            for offset in range(1, len(rhythm.steps)):
                tied = rhythm.steps[(index + offset) % len(rhythm.steps)]
                if not isinstance(tied, TieStep):
                    break
                tied_duration = Fraction(tied.duration.removesuffix(" beat"))
                final_gate = max(attack_gate, held + tied_duration * gate)
                held += tied_duration
    else:
        duration = Fraction(rhythm.step.removesuffix(" beat"))
        repeats = int(allows_step(rhythm, index))
        attack_gate = final_gate = duration * gate
    return RhythmDecision(
        duration=duration, repeats=repeats, gate=attack_gate, final_gate=final_gate
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
