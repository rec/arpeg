import json
from fractions import Fraction
from pathlib import Path

import pytest
from ufor.arpeggiator import ArpeggiatorScore, LatchedBank

from arpeg.live import LiveArpeggiator


def _profile() -> ArpeggiatorScore:
    case = json.loads(Path("conformance/held-chord.json").read_text())
    return ArpeggiatorScore.model_validate(case["profile"])


def test_live_notes_follow_input_and_release_when_bank_empties() -> None:
    arp = LiveArpeggiator(_profile())
    assert arp.note_on(Fraction(0), 60, 100) == []
    assert [(e.kind, e.key, e.at) for e in arp.advance(Fraction(0))] == [
        ("on", 60, Fraction(0))
    ]
    assert arp.note_on(Fraction(1, 8), 64, 90) == []
    assert [(e.kind, e.key, e.at) for e in arp.advance(Fraction(1, 4))] == [
        ("off", 60, Fraction(1, 5)),
        ("on", 64, Fraction(1, 4)),
    ]
    assert arp.note_off(Fraction(3, 10), 60) == []
    assert [(e.kind, e.key, e.at) for e in arp.note_off(Fraction(7, 20), 64)] == [
        ("off", 64, Fraction(7, 20))
    ]
    assert arp.advance(Fraction(1, 2)) == []


def test_simultaneous_note_ons_join_first_step() -> None:
    arp = LiveArpeggiator(_profile())
    arp.note_on(Fraction(0), 64, 90)
    arp.note_on(Fraction(0), 60, 100)
    events = arp.advance(Fraction(0))
    assert [(e.kind, e.key, e.velocity) for e in events] == [("on", 60, 100)]


def test_stop_releases_only_sounding_outputs() -> None:
    arp = LiveArpeggiator(_profile())
    arp.note_on(Fraction(0), 60, 100)
    arp.advance(Fraction(0))
    assert [(e.kind, e.key, e.at) for e in arp.stop(Fraction(1, 10))] == [
        ("off", 60, Fraction(1, 10))
    ]
    assert arp.advance(Fraction(1, 4)) == []


def test_late_input_and_unsupported_bank_fail_explicitly() -> None:
    arp = LiveArpeggiator(_profile())
    arp.advance(Fraction(1))
    with pytest.raises(ValueError, match="backwards"):
        arp.note_on(Fraction(0), 60, 100)
    profile = _profile()
    latched = profile.model_copy(
        update={"body": profile.body.model_copy(update={"bank": LatchedBank()})}
    )
    with pytest.raises(ValueError, match="held banks"):
        LiveArpeggiator(latched)
