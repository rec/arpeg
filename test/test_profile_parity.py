from pathlib import Path

from ufor.codec import parse_score


def test_canonical_profile_is_shared_with_the_native_shell() -> None:
    simple = Path("conformance/up.toml").read_text()
    expanded = Path("conformance/up-expanded.toml").read_text()
    assert parse_score(simple) == parse_score(expanded)
