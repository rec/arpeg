import json
from fractions import Fraction
from pathlib import Path

import pytest
from ufor.arpeggiator import ArpeggiatorScore, HistoryBank, LatchedBank
from ufor.codec import parse_score

from arpeg.live import LiveArpeggiator


def _profile() -> ArpeggiatorScore:
    case = json.loads(Path("conformance/held-chord.json").read_text())
    return ArpeggiatorScore.model_validate(case["profile"])


def test_live_notes_follow_input_and_release_when_bank_empties() -> None:
    arp = LiveArpeggiator(profile=_profile())
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
    arp = LiveArpeggiator(profile=_profile())
    arp.note_on(Fraction(0), 64, 90)
    arp.note_on(Fraction(0), 60, 100)
    events = arp.advance(Fraction(0))
    assert [(e.kind, e.key, e.velocity) for e in events] == [("on", 60, 100)]


def test_stop_releases_only_sounding_outputs() -> None:
    arp = LiveArpeggiator(profile=_profile())
    arp.note_on(Fraction(0), 60, 100)
    arp.advance(Fraction(0))
    assert [(e.kind, e.key, e.at) for e in arp.stop(Fraction(1, 10))] == [
        ("off", 60, Fraction(1, 10))
    ]
    assert arp.advance(Fraction(1, 4)) == []


def test_late_input_and_unsupported_bank_fail_explicitly() -> None:
    arp = LiveArpeggiator(profile=_profile())
    arp.advance(Fraction(1))
    with pytest.raises(ValueError, match="backwards"):
        arp.note_on(Fraction(0), 60, 100)
    profile = _profile()
    history = profile.model_copy(
        update={"body": profile.body.model_copy(update={"bank": HistoryBank()})}
    )
    with pytest.raises(ValueError, match="held or latched"):
        LiveArpeggiator(profile=history)


def test_latched_replace_groups_overlapping_keys_and_preserves_current_gate() -> None:
    profile = _profile()
    profile = profile.model_copy(
        update={"body": profile.body.model_copy(update={"bank": LatchedBank()})}
    )
    arp = LiveArpeggiator(profile=profile)
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
    arp = LiveArpeggiator(profile=profile)
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
    arp = LiveArpeggiator(profile=profile)
    arp.note_on(Fraction(0), 60, 100)
    arp.advance(Fraction(0))
    arp.note_off(Fraction(1, 8), 60)
    assert [(e.kind, e.key, e.at) for e in arp.note_on(Fraction(3, 16), 60, 100)] == [
        ("off", 60, Fraction(3, 16))
    ]
    assert arp.advance(Fraction(1, 4)) == []


@pytest.mark.parametrize(
    ("retrigger", "expected"),
    [("on_empty", 67), ("bank_edit", 60)],
)
def test_bank_edit_retrigger_restarts_selection_without_moving_grid(
    retrigger: str, expected: int
) -> None:
    profile = _profile()
    profile = ArpeggiatorScore.model_validate(
        {
            **profile.model_dump(),
            "body": {**profile.body.model_dump(), "retrigger": retrigger},
        }
    )
    arp = LiveArpeggiator(profile=profile)
    for key in (60, 64, 67):
        arp.note_on(Fraction(0), key, 100)
    arp.advance(Fraction(1, 4))
    arp.note_on(Fraction(3, 8), 72, 100)
    assert [(e.kind, e.key) for e in arp.advance(Fraction(1, 2))][-1] == (
        "on",
        expected,
    )


def test_latched_add_retriggers_on_bank_edit_but_not_key_release() -> None:
    profile = _profile()
    profile = ArpeggiatorScore.model_validate(
        {
            **profile.model_dump(),
            "body": {
                **profile.body.model_dump(),
                "bank": {"kind": "latched", "update": "add"},
                "retrigger": "bank_edit",
            },
        }
    )
    arp = LiveArpeggiator(profile=profile)
    for key in (60, 64, 67):
        arp.note_on(Fraction(0), key, 100)
    arp.advance(Fraction(1, 4))
    for key in (60, 64, 67):
        arp.note_off(Fraction(3, 8), key)
    assert [(e.kind, e.key) for e in arp.advance(Fraction(1, 2))][-1] == (
        "on",
        67,
    )


def test_toggle_keeps_same_time_duplicate_pitches_distinct() -> None:
    profile = _profile()
    profile = profile.model_copy(
        update={
            "body": profile.body.model_copy(
                update={"bank": LatchedBank(update="toggle")}
            )
        }
    )
    arp = LiveArpeggiator(profile=profile)
    arp.note_on(Fraction(0), 60, 90)
    arp.note_on(Fraction(0), 60, 100)
    assert [(e.kind, e.source_id, e.velocity) for e in arp.advance(Fraction(0))] == [
        ("on", 0, 90)
    ]
    assert [(e.kind, e.source_id, e.velocity) for e in arp.advance(Fraction(1, 4))] == [
        ("off", 0, 0),
        ("on", 1, 100),
    ]


def test_clear_releases_latched_output_without_losing_input_pairing() -> None:
    profile = _profile()
    profile = profile.model_copy(
        update={"body": profile.body.model_copy(update={"bank": LatchedBank()})}
    )
    arp = LiveArpeggiator(profile=profile)
    arp.note_on(Fraction(0), 60, 100)
    arp.advance(Fraction(0))
    assert [(e.kind, e.at) for e in arp.clear(Fraction(1, 8))] == [
        ("off", Fraction(1, 8))
    ]
    assert arp.note_off(Fraction(3, 16), 60) == []
    assert arp.advance(Fraction(1, 4)) == []


def test_saved_walk_continues_the_same_random_sequence() -> None:
    profile = parse_score(Path("conformance/weighted-walk.toml").read_text())
    assert isinstance(profile, ArpeggiatorScore)
    arp = LiveArpeggiator(profile=profile)
    for key in (60, 64, 67):
        arp.note_on(Fraction(0), key, 100)
    arp.advance(Fraction(1, 4))
    restored = LiveArpeggiator.model_validate_json(arp.model_dump_json())
    assert restored.advance(Fraction(3)) == arp.advance(Fraction(3))


def test_saved_alternating_traversal_preserves_its_direction() -> None:
    profile = parse_score(Path("conformance/alternating.toml").read_text())
    assert isinstance(profile, ArpeggiatorScore)
    arp = LiveArpeggiator(profile=profile)
    for key in (60, 64, 67):
        arp.note_on(Fraction(0), key, 100)
    arp.advance(Fraction(3, 4))
    restored = LiveArpeggiator.model_validate_json(arp.model_dump_json())
    assert restored.advance(Fraction(2)) == arp.advance(Fraction(2))


@pytest.mark.parametrize(
    "fixture",
    ["live-classic", "euclidean", "custom-steps", "chance-walk", "alternating"],
)
def test_live_classic_matches_shared_python_rust_traces(fixture: str) -> None:
    cases = json.loads(Path(f"conformance/{fixture}.json").read_text())["cases"]
    for case in cases:
        bank = (
            {"kind": "held"}
            if case["bank"] == "held"
            else {"kind": "latched", "update": case["bank"]}
        )
        profile = _profile()
        profile = ArpeggiatorScore.model_validate(
            {
                **profile.model_dump(),
                "name": case.get("profile_name", profile.name),
                "body": {
                    **profile.body.model_dump(),
                    "probability": case.get("probability", "1"),
                    "seed": case.get("seed"),
                    "selection": case.get("selection", {"kind": "ascending"}),
                    "bank": bank,
                    "retrigger": case["retrigger"],
                    "rhythm": case.get("rhythm", profile.body.rhythm.model_dump()),
                    "gate": case.get("gate", profile.body.gate),
                },
            }
        )
        for polling in (False, True):
            arp = LiveArpeggiator(profile=profile)
            events = []
            for action in case["actions"]:
                at = Fraction(action[1])
                if polling:
                    while arp.now + Fraction(1, 17) < at:
                        events.extend(arp.advance(arp.now + Fraction(1, 17)))
                if action[0] == "on":
                    events.extend(arp.note_on(at, action[2], action[3]))
                elif action[0] == "off":
                    events.extend(arp.note_off(at, action[2]))
                elif action[0] == "advance":
                    events.extend(arp.advance(at))
                elif action[0] == "stop":
                    events.extend(arp.stop(at))
                else:
                    events.extend(arp.clear(at))
            actual = [
                [str(e.at), e.kind, e.id, e.source_id, e.key, e.velocity]
                for e in events
            ]
            assert actual == case["expected"], case["name"]
