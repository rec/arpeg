from fractions import Fraction
from pathlib import Path
from tomllib import loads
from typing import Literal

import mido
import pytest
from ufor.arpeggiator import ArpeggiatorScore
from ufor.codec import parse_score

from arpeg.clock import ClockMode, TransportClock
from arpeg.midi import MidiPlayer


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/transport.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
def test_shared_transport_trace(case: dict[str, object]) -> None:
    profile = parse_score(Path(f"conformance/{case['profile']}.toml").read_text())
    assert isinstance(profile, ArpeggiatorScore)
    player = MidiPlayer(
        profile=profile,
        clock=TransportClock(
            mode=ClockMode(str(case["mode"])),
            timeout_us=int(str(case.get("timeout_us", 500_000))),
        ),
    )
    actions = case["actions"]
    assert isinstance(actions, list)
    for action in actions:
        at = action["at"] * 1000
        if "data" in action:
            source: Literal["notes", "clock", "both"] = action.get("source", "both")
            output = player.accept(
                at, mido.Message.from_bytes(action["data"]), source=source
            )
        elif "tempo" in action:
            output = player.set_tempo(at, action["tempo"])
        else:
            output = player.advance(at)
        assert [m.bytes() for m in output] == action["output"], action
        assert player.clock.beat == Fraction(action["beat"]), action
        assert player.clock.active == action["active"], action
        # Restoring the full player must preserve transport, input and owned output.
        player = MidiPlayer.model_validate_json(player.model_dump_json())
