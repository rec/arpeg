"""Octave displacement for repeated chord inversions, preserving source ranks."""


def inversion_offsets(keys: list[int], count: int, period: int = 12) -> list[int]:
    offsets = [0] * len(keys)
    if not keys or not count:
        return offsets
    direction = 1 if count > 0 else -1
    ordered = sorted(range(len(keys)), key=lambda i: (direction * keys[i], i))
    edge = direction * keys[ordered[-1]]
    end = edge
    first: list[int] = []
    for i in ordered:
        end += (direction * keys[i] - end - 1) % period + 1
        first.append(end)
    span = end - edge
    cycles, remainder = divmod(abs(count), len(keys))
    for j, i in enumerate(ordered):
        if cycles or j < remainder:
            pitch = (
                first[j] + (cycles - 1 + int(j < remainder)) * span
                if cycles
                else first[j]
            )
            offsets[i] = direction * pitch - keys[i]
    return offsets
