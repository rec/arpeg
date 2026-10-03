import json
from fractions import Fraction
from pathlib import Path

import pytest
from ufor.arpeggiator import ArpeggiatorScore, HistoryBank, LatchedBank

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
    history = profile.model_copy(
        update={"body": profile.body.model_copy(update={"bank": HistoryBank()})}
    )
    with pytest.raises(ValueError, match="held or latched"):
        LiveArpeggiator(history)


def test_latched_replace_groups_overlapping_keys_and_preserves_current_gate() -> None:
    profile = _profile()
    profile = profile.model_copy(
        update={"body": profile.body.model_copy(update={"bank": LatchedBank()})}
    )
    arp = LiveArpeggiator(profile)
    arp.note_on(Fraction(0), 60, 100)
    arp.note_on(Fraction(0), 64, 90)
    assert [(e.kind, e.key) for e in arp.advance(Fraction(0))] == [("on", 60)]
    arp.note_off(Fraction(1, 8), 60)
    arp.note_off(Fraction(1, 8), 64)
    assert [(e.kind, e.key) for e in arp.advance(Fraction(1, 4))] == [
        ("off", 60),
        ("on", 64),
    ]
    assert arp.note_on(Fraction(3, 8), 67, 80) == []
    assert [(e.kind, e.key) for e in arp.advance(Fraction(1, 2))] == [
        ("off", 64),
        ("on", 67),
    ]


def test_latched_add_retains_released_notes() -> None:
    profile = _profile()
    profile = profile.model_copy(
        update={
            "body": profile.body.model_copy(update={"bank": LatchedBank(update="add")})
        }
    )
    arp = LiveArpeggiator(profile)
    arp.note_on(Fraction(0), 60, 100)
    arp.advance(Fraction(0))
    arp.note_off(Fraction(1, 8), 60)
    arp.note_on(Fraction(3, 16), 64, 90)
    assert [(e.kind, e.key) for e in arp.advance(Fraction(1, 4))] == [
        ("off", 60),
        ("on", 64),
    ]


def test_latched_toggle_clears_bank_and_releases_its_output() -> None:
    profile = _profile()
    profile = profile.model_copy(
        update={
            "body": profile.body.model_copy(
                update={"bank": LatchedBank(update="toggle")}
            )
        }
    )
    arp = LiveArpeggiator(profile)
    arp.note_on(Fraction(0), 60, 100)
    arp.advance(Fraction(0))
    arp.note_off(Fraction(1, 8), 60)
    assert [(e.kind, e.key, e.at) for e in arp.note_on(Fraction(3, 16), 60, 100)] == [
        ("off", 60, Fraction(3, 16))
    ]
    assert arp.advance(Fraction(1, 4)) == []
