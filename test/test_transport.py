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
    [
        c
        for f in ("transport", "performance")
        for c in loads(Path(f"conformance/{f}.toml").read_text())["cases"]
    ],
    ids=lambda c: c["name"],
)
@pytest.mark.parametrize("poll_us", [None, 1000])
def test_shared_transport_trace(case: dict[str, object], poll_us: int | None) -> None:
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
    previous_at = 0
    for action in actions:
        at = action["at"] * 1000
        output: list[mido.Message] = []
        if poll_us is not None:
            for tick in range(previous_at + poll_us * 1000, at, poll_us * 1000):
                output.extend(player.advance(tick))
        if "data" in action:
            source: Literal["notes", "clock", "both"] = action.get("source", "both")
            output.extend(
                player.accept(
                    at, mido.Message.from_bytes(action["data"]), source=source
                )
            )
        elif "tempo" in action:
            output.extend(player.set_tempo(at, action["tempo"]))
        elif "clear" in action:
            output.extend(player.clear(at))
        else:
            output.extend(player.advance(at))
        assert [m.bytes() for m in output] == action["output"], action
        assert player.clock.beat == Fraction(action["beat"]), action
        assert player.clock.active == action["active"], action
        # Restoring the full player must preserve transport, input and owned output.
        player = MidiPlayer.model_validate_json(player.model_dump_json())
        previous_at = at
