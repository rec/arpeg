from pathlib import Path

import pytest
from ufor.codec import parse_score, score_toml


def test_canonical_profile_is_shared_with_the_native_shell() -> None:
    simple = Path("conformance/up.toml").read_text()
    expanded = Path("conformance/up-expanded.toml").read_text()
    assert score_toml(parse_score(simple)) == expanded


def test_live_latch_profile_loads_in_python() -> None:
    profile = parse_score(Path("conformance/live-latch.toml").read_text())
    assert profile.kind == "arpeggiator"
    assert profile.body.retrigger == "bank_edit"


def test_custom_steps_profile_round_trips() -> None:
    profile = parse_score(Path("conformance/custom-steps.toml").read_text())
    assert parse_score(score_toml(profile)) == profile


def test_weighted_walk_profile_round_trips() -> None:
    profile = parse_score(Path("conformance/weighted-walk.toml").read_text())
    assert parse_score(score_toml(profile)) == profile


def test_alternating_profile_round_trips() -> None:
    profile = parse_score(Path("conformance/alternating.toml").read_text())
    assert parse_score(score_toml(profile)) == profile


@pytest.mark.parametrize("name", ["inside-out", "outside-in", "index-pattern"])
def test_center_edge_profile_round_trips(name: str) -> None:
    profile = parse_score(Path(f"conformance/{name}.toml").read_text())
    assert parse_score(score_toml(profile)) == profile
