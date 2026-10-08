import json
from pathlib import Path

from ufor.arpeggiator import Euclidean

from arpeg.rhythm import allows_step, starts_cycle


def test_euclidean_rotations_match_canonical_masks() -> None:
    masks = json.loads(Path("conformance/euclidean.json").read_text())["masks"]
    for rotation in range(-8, 16):
        rhythm = Euclidean(steps=8, pulses=3, rotation=rotation, step="1/4 beat")
        assert [i for i in range(16) if starts_cycle(rhythm, i)] == [0, 8]
        assert "".join(str(int(allows_step(rhythm, i))) for i in range(16)) == (
            masks[rotation % 8] * 2
        )


def test_euclidean_pulse_counts_and_spacing() -> None:
    for steps in range(1, 17):
        for pulses in range(steps + 1):
            for rotation in range(steps):
                rhythm = Euclidean(
                    steps=steps, pulses=pulses, rotation=rotation, step="1/3 beat"
                )
                hits = [i for i in range(steps) if allows_step(rhythm, i)]
                assert len(hits) == pulses
                if hits:
                    gaps = [
                        b - a
                        for a, b in zip(hits, hits[1:] + [hits[0] + steps], strict=True)
                    ]
                    assert max(gaps) - min(gaps) <= 1
