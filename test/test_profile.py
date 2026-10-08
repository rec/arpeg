from pathlib import Path
from tomllib import loads

import pytest

from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/profile-headers.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
def test_profile_header_defaults(case: dict[str, str], tmp_path: Path) -> None:
    path = tmp_path / case["path"] if "path" in case else None
    text = case["text"]
    if path is not None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        text = path.read_text()
    if "error" in case:
        with pytest.raises(ValueError, match=case["error"]):
            parse_profile(text, path)
    else:
        profile = parse_profile(text, path)
        assert profile.kind == "arpeggiator"
        assert profile.name == case["expected_name"]
