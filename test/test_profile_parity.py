from pathlib import Path

from ufor.codec import parse_score, score_toml


def test_canonical_profile_is_shared_with_the_native_shell() -> None:
    simple = Path("conformance/up.toml").read_text()
    expanded = Path("conformance/up-expanded.toml").read_text()
    assert score_toml(parse_score(simple)) == expanded
