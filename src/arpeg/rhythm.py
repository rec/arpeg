"""Canonical rhythm masks on the arpeggiator's local step clock."""

from ufor.arpeggiator import Euclidean, Grid


def allows_step(rhythm: Grid | Euclidean, index: int) -> bool:
    """Positive rotation moves hits later; zero pulses produces silence."""
    if isinstance(rhythm, Grid):
        return True
    phase = (index - rhythm.rotation) % rhythm.steps
    return (phase * rhythm.pulses) % rhythm.steps < rhythm.pulses
