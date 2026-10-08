"""Read arpeggiator presets with file-aware header defaults."""

from pathlib import Path
from tomllib import loads

from ufor.arpeggiator import ArpeggiatorScore


def parse_profile(text: str, path: Path | None = None) -> ArpeggiatorScore:
    """Use the source filename's stem only when the preset omits its name."""
    data = loads(text)
    data.setdefault("kind", "arpeggiator")
    if "name" not in data:
        if path is None:
            raise ValueError("preset requires name when no source file is supplied")
        data["name"] = path.stem
    return ArpeggiatorScore.model_validate(data)
