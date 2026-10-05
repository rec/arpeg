import json
from pathlib import Path

from arpeg.chance import draw_below


def test_named_draws_match_shared_vectors() -> None:
    draws = json.loads(Path("conformance/chance-walk.json").read_text())["draws"]
    for lane, bound in (("probability", 3), ("walk", 5)):
        assert [
            draw_below(42, "weighted-walk", lane, 3, i, bound) for i in range(12)
        ] == draws[lane]


def test_single_outcome_always_returns_zero() -> None:
    assert draw_below(-42, "chord", "walk", 7, 99, 1) == 0
