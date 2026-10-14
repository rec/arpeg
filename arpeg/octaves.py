"""Enumerate source/layer ranks without changing source notes."""

from ufor.arpeggiator import Octaves, OctaveTraversal


def expanded_indices(
    size: int, octaves: Octaves, reverse: bool = False
) -> list[tuple[int, int]]:
    layers = range(len(octaves.offsets))
    if reverse:
        layers = range(len(octaves.offsets) - 1, -1, -1)
    if octaves.traversal == OctaveTraversal.notes_first:
        return [(i, j) for j in layers for i in range(size)]
    return [(i, j) for i in range(size) for j in layers]
