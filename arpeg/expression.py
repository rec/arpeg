"""Lane ownership and exact normalized expression at the MIDI destination."""

from fractions import Fraction
from math import floor

from ufor.arpeggiator import Expression, ExpressionLane


def expression_lane(data: list[int]) -> ExpressionLane | None:
    match data:
        case [0xB0, 2, _]:
            return ExpressionLane.breath
        case [0xE0, _, _]:
            return ExpressionLane.bend
        case [0xD0, _]:
            return ExpressionLane.pressure
        case _:
            return None


def motion_message(lane: ExpressionLane, value: Fraction) -> list[int]:
    if lane == ExpressionLane.bend:
        wheel = floor(8192 + value * (8192 if value < 0 else 8191) + Fraction(1, 2))
        return [0xE0, wheel & 127, wheel >> 7]
    level = floor(value * 127 + Fraction(1, 2))
    return [0xB0, 2, level] if lane == ExpressionLane.breath else [0xD0, level]


def entry_messages(
    profile: Expression,
    current: dict[int, list[int]],
    motion: dict[ExpressionLane, Fraction],
) -> list[list[int]]:
    output: list[list[int]] = []
    for lane, status in _LANES:
        source = profile.lanes.get(lane, profile.source)
        if source == "current" and status in current:
            output.append(current[status])
        elif source == "motion" and lane in motion:
            output.append(motion_message(lane, motion[lane]))
    return output


_LANES = [
    (ExpressionLane.breath, 0xB0),
    (ExpressionLane.bend, 0xE0),
    (ExpressionLane.pressure, 0xD0),
]
