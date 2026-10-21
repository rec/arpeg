"""Reproducible, independently named random choices without shared RNG state."""

from ufor.arpeggiator import IntervalEnvelope, Transition


def transition_rank(
    selection: Transition,
    size: int,
    previous: int | None,
    seed: int,
    name: str,
    revision: int,
    decision: int,
) -> tuple[int | None, int]:
    """Choose an expanded rank, consuming a draw only for an admitted transition."""
    if previous is None and selection.start == "lowest":
        return 0, decision
    row = (
        selection.weights[previous]
        if previous is not None and previous < len(selection.weights)
        else []
    )
    weights = [
        1 if previous is None else row[i] if i < len(row) else selection.missing_weight
        for i in range(size)
    ]
    if not any(weights):
        if selection.blocked == "rest":
            return None, decision
        weights = [1] * size
    chosen = draw_below(seed, name, "transition", revision, decision, sum(weights))
    index = 0
    while chosen >= weights[index]:
        chosen -= weights[index]
        index += 1
    return index, decision + 1


def interval_rank(
    selection: IntervalEnvelope,
    size: int,
    previous: int | None,
    seed: int,
    name: str,
    revision: int,
    step: int,
) -> int | None:
    """Choose a signed movement using the absolute rhythm step's range."""
    if previous is None:
        return 0
    interval = selection.ranges[step % len(selection.ranges)]
    low, high = interval.min, interval.max
    if selection.boundary == "rest":
        low, high = max(low, -previous), min(high, size - 1 - previous)
        if low > high:
            return None
    move = low + draw_below(
        seed, name, "interval_envelope", revision, step, high - low + 1
    )
    return (previous + move) % size


def draw_below(
    seed: int, name: str, lane: str, revision: int, decision: int, bound: int
) -> int:
    """Draw uniformly below a positive bound using the documented v1 mixer."""
    bits = (bound - 1).bit_length()
    retry = 0
    while True:
        value = 0
        for chunk in range((bits + 63) // 64):
            text = (
                f"arpeg-v1:{seed}:{len(name.encode())}:{name}:{lane}:"
                f"{revision}:{decision}:{retry}:{chunk}"
            )
            word = 0xCBF29CE484222325
            for byte in text.encode():
                word = ((word ^ byte) * 0x100000001B3) & _MASK
            word = ((word ^ (word >> 30)) * 0xBF58476D1CE4E5B9) & _MASK
            word = ((word ^ (word >> 27)) * 0x94D049BB133111EB) & _MASK
            value |= (word ^ (word >> 31)) << (chunk * 64)
        value &= (1 << bits) - 1
        if value < bound:
            return value
        retry += 1


_MASK = (1 << 64) - 1
