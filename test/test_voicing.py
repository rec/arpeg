from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest

from arpeg.live import LiveArpeggiator
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/voicing.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
def test_voicing_preserves_sources_and_selector_progress(
    case: dict[str, object],
) -> None:
    profile = parse_profile(Path("conformance/up.toml").read_text())
    settings = profile.body.model_dump()
    settings.update(
        {
            k: v
            for k, v in case.items()
            if k
            in (
                "inversion",
                "selection",
                "octaves",
                "scale",
                "voicing",
                "selection_offset",
            )
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


def test_latched_edits_reassign_voicing_without_changing_pending_repeats() -> None:
    profile = parse_profile("""name="voicing"
title="Voicing"
[body]
bank={kind="latched",update="add"}
voicing=[0,1,0]
retrigger="bank_edit"
rhythm={kind="pattern",steps=[{kind="hit",duration="1 beat",repeats=3}]}
""")
    arp = LiveArpeggiator(profile=profile)
    for k, v in [(60, 100), (64, 90), (67, 80)]:
        arp.note_on(Fraction(0), k, v)
    assert [e.key for e in arp.advance(Fraction(0)) if e.kind == "on"] == [60]
    arp.note_off(Fraction(1, 8), 60)
    arp.note_on(Fraction(1, 6), 55, 70)
    assert [e.key for e in arp.advance(Fraction(2, 3)) if e.kind == "on"] == [60, 60]
    assert [e.key for e in arp.advance(Fraction(1)) if e.kind == "on"] == [55]
    assert [e.key for e in arp.advance(Fraction(2)) if e.kind == "on"] == [55, 55, 64]


def test_ties_keep_the_voiced_pitch_after_chord_edits() -> None:
    profile = parse_profile("""name="voicing"
title="Voicing"
[body]
voicing=[1,0]
[body.rhythm]
kind="pattern"
steps=[{kind="hit",duration="1 beat"},{kind="tie",duration="1 beat"}]
""")
    arp = LiveArpeggiator(profile=profile)
    arp.note_on(Fraction(0), 60, 100)
    assert [(e.kind, e.key) for e in arp.advance(Fraction(0))] == [("on", 72)]
    assert arp.note_on(Fraction(1, 2), 55, 90) == []
    assert arp.advance(Fraction(1)) == []
    assert [(e.kind, e.key) for e in arp.stop(Fraction(3, 2))] == [("off", 72)]
