from pathlib import Path

from ufor.codec import parse_score, score_toml


def test_canonical_profile_is_shared_with_the_native_shell() -> None:
    simple = Path("conformance/up.toml").read_text()
    expanded = Path("conformance/up-expanded.toml").read_text()
    assert score_toml(parse_score(simple)) == expanded


def test_live_latch_profile_loads_in_python() -> None:
    profile = parse_score(Path("conformance/live-latch.toml").read_text())
    assert profile.kind == "arpeggiator"
    assert profile.body.retrigger == "bank_edit"
