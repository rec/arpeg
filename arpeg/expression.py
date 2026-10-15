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


def envelfo_message(lane: ExpressionLane, value: Fraction) -> list[int]:
    if lane == ExpressionLane.bend:
        wheel = floor(8192 + value * (8192 if value < 0 else 8191) + Fraction(1, 2))
        return [0xE0, wheel & 127, wheel >> 7]
    level = floor(value * 127 + Fraction(1, 2))
    return [0xB0, 2, level] if lane == ExpressionLane.breath else [0xD0, level]


def entry_messages(
    profile: Expression,
    current: dict[int, list[int]],
    envelfo: dict[ExpressionLane, Fraction],
) -> list[list[int]]:
    output: list[list[int]] = []
    for lane, status in _LANES:
        source = profile.lanes.get(lane, profile.source)
        if source == "current" and status in current:
            output.append(current[status])
        elif source == "envelfo" and lane in envelfo:
            output.append(envelfo_message(lane, envelfo[lane]))
    return output


_LANES = [
    (ExpressionLane.breath, 0xB0),
    (ExpressionLane.bend, 0xE0),
    (ExpressionLane.pressure, 0xD0),
]


def tuning_message(
    data: list[int], offset: float, bend_range: float | None
) -> list[int]:
    if bend_range is None or data[0] != 0xE0:
        return data
    wheel = data[1] + 128 * data[2] - 8192
    value = wheel / (8192 if wheel < 0 else 8191) + offset / bend_range
    if not -1 <= value <= 1:
        raise RuntimeError(
            "tuning plus expression exceeds the configured MIDI bend range"
        )
    level = floor(8192 + value * (8192 if value < 0 else 8191) + 0.5)
    return [0xE0, level & 127, level >> 7]
