import json
from pathlib import Path

import pytest

from arpeg.marked_sample import MarkedSample


def _bank() -> MarkedSample:
    case = json.loads(Path("conformance/marked-sample.json").read_text())
    return MarkedSample.model_validate(case)


def test_markers_lower_to_exhaustive_regions_without_inventing_pitch() -> None:
    phrase = _bank().phrase()
    assert [
        (
            n.note_id,
            n.selection_key,
            n.key,
            n.region.start_frame,
            n.region.end_frame,
        )
        for n in phrase.notes
        if n.region is not None
    ] == [
        ("a", 60, None, 0, 12_000),
        ("b", 62, None, 12_000, 32_000),
        ("c", 64, None, 32_000, 48_000),
    ]
    assert phrase.events == []
    assert phrase.end_tick == 48_000
    case = json.loads(Path("conformance/marked-sample.json").read_text())
    for order in ("ascending", "descending"):
        assert [
            [n.note_id, n.region.start_frame, n.region.end_frame]
            for n in _bank().select(order)
            if n.region is not None
        ] == case[order]
    assert [n.note_id for n in _bank().select("reverse_played", cycles=2)] == [
        "c",
        "b",
        "a",
        "c",
        "b",
        "a",
    ]


def test_exhaustive_bank_requires_an_explicit_prefix_marker() -> None:
    with pytest.raises(ValueError, match="frame zero"):
        MarkedSample.model_validate(
            {
                **_bank().model_dump(),
                "markers": [{"note_id": "late", "selection_key": 60, "at_frame": 1}],
            }
        )
