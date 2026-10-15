from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest

from arpeg.inversion import inversion_offsets
from arpeg.live import LiveArpeggiator
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/inversion.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
def test_inversion_preserves_sources_and_uses_voiced_ranks(
    case: dict[str, object],
) -> None:
    profile = parse_profile(Path("conformance/up.toml").read_text())
    settings = profile.body.model_dump()
    settings.update(
        {
            k: v
            for k, v in case.items()
            if k in ("inversion", "selection", "octaves", "scale")
        }
    )
    if "limits" in case:
        settings["register_limits"] = case["limits"]
    profile = profile.model_copy(
        update={"body": type(profile.body).model_validate(settings)}
    )
    arp = LiveArpeggiator(profile=profile)
    notes, keys, sources = case["notes"], case["keys"], case["sources"]
    assert (
        isinstance(notes, list) and isinstance(keys, list) and isinstance(sources, list)
    )
    for n in notes:
        arp.note_on(Fraction(0), *n)
    for i, key in enumerate(keys):
        attacks = [e for e in arp.advance(Fraction(i, 4)) if e.kind == "on"]
        assert [e.key for e in attacks] == ([] if key == -1 else [key])
        if attacks:
            assert attacks[0].source_id == sources[i]
            assert attacks[0].velocity == notes[sources[i]][1]
        arp = LiveArpeggiator.model_validate_json(arp.model_dump_json())


@pytest.mark.parametrize("count", [-19, -4, -1, 0, 1, 4, 19])
def test_direct_inversion_matches_successive_chord_rotations(count: int) -> None:
    keys = [36, 60, 64, 64, 79]
    expected = list(keys)
    for _ in range(abs(count)):
        rank = min(
            range(len(keys)),
            key=lambda i: (expected[i] if count > 0 else -expected[i], i),
        )
        if count > 0:
            expected[rank] += ((max(expected) - expected[rank]) // 12 + 1) * 12
        else:
            expected[rank] -= ((expected[rank] - min(expected)) // 12 + 1) * 12
    assert [
        k + o for k, o in zip(keys, inversion_offsets(keys, count), strict=True)
    ] == expected


def test_empty_chord_has_no_inversion() -> None:
    assert inversion_offsets([], 10) == []


def test_latched_edits_preserve_repeats_and_recalculate_inversion() -> None:
    profile = parse_profile("""name="inversion"
title="Inversion"
[body]
bank={kind="latched",update="add"}
inversion=1
retrigger="bank_edit"
rhythm={kind="pattern",steps=[{kind="hit",duration="1 beat",repeats=3}]}
""")
    arp = LiveArpeggiator(profile=profile)
    arp.note_on(Fraction(0), 60, 100)
    arp.note_on(Fraction(0), 64, 90)
    arp.note_on(Fraction(0), 67, 80)
    assert [e.key for e in arp.advance(Fraction(0)) if e.kind == "on"] == [64]
    arp.note_off(Fraction(1, 8), 60)
    arp.note_on(Fraction(1, 6), 55, 70)
    assert [e.key for e in arp.advance(Fraction(2, 3)) if e.kind == "on"] == [64, 64]
    assert [e.key for e in arp.advance(Fraction(1)) if e.kind == "on"] == [60]
