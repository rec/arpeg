"""The same occurrence engine supports opaque text and musical adapters."""

import json
from fractions import Fraction
from pathlib import Path

import pytest
from ufor.arpeggiator import LatchedBank

from arpeg.engine import EngineSettings, TokenArpeggiator


@pytest.mark.parametrize("polling", [False, True])
def test_text_matches_shared_rust_traces_and_restores_pending_occurrences(
    polling: bool,
) -> None:
    fixture = json.loads(Path("conformance/tokens.json").read_text())
    for case in fixture["cases"]:
        settings = EngineSettings(gate=Fraction(case["gate"]))
        if case["latch"]:
            settings = settings.model_copy(update={"bank": LatchedBank()})
        engine = TokenArpeggiator(settings=settings)
        events = []
        for action, time, *args in case["actions"]:
            at = Fraction(time)
            if polling:
                while engine.now + Fraction(1, 17) < at:
                    events.extend(engine.advance(engine.now + Fraction(1, 17)))
            if action == "press":
                events.extend(
                    engine.press(at, args[0], args[1], case["tokens"][args[2]])
                )
            elif action == "release":
                events.extend(engine.release(at, args[0]))
            elif action == "advance":
                events.extend(engine.advance(at))
            else:
                events.extend(engine.stop(at))
            engine = TokenArpeggiator.model_validate_json(engine.model_dump_json())
        assert [
            [str(e.at), e.kind, e.id, e.source_id, e.value] for e in events
        ] == case["expected"], case["name"]


def test_text_does_not_accept_musical_controls() -> None:
    from ufor.arpeggiator_ports import ArpeggiatorControl

    engine = TokenArpeggiator()
    with pytest.raises(ValueError, match="not supported"):
        engine.control(Fraction(0), ArpeggiatorControl(port="transposition", value=12))
