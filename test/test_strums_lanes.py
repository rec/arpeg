from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest
from meedee import codecs
from ufor.arpeggiator import ArpeggiatorScore

from arpeg.clock import ClockMode, TransportClock
from arpeg.midi import MidiPlayer, decode_message


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/strums-lanes.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
@pytest.mark.parametrize("polling", [False, True])
def test_strums_and_lanes_match_shared_traces_and_snapshots(
    case: dict[str, object], polling: bool
) -> None:
    player = _player(case["body"])
    for key, velocity in case["notes"]:
        player.accept(0, decode_message([144, key, velocity]))
    end = int(Fraction(case["through"]) * 500_000_000)
    notes = []
    wire = []
    for at in [*range(0, end, 17_000_000), end] if polling else [end]:
        wire.extend(list(codecs.encode(m)) for m in player.advance(at))
        batch = player.take_events()
        notes.extend(batch.notes)
        assert not batch.exhausted
        player = MidiPlayer.model_validate_json(player.model_dump_json())
    assert [
        [
            str(n.at),
            "on" if n.port == "note_start" else "off",
            n.channel,
            n.key,
            n.velocity,
        ]
        for n in notes
    ] == case["events"]
    assert wire == [
        [(143 if e[1] == "on" else 127) + e[2], e[3], e[4]] for e in case["events"]
    ]


def test_strum_chord_snapshot_survives_edits_and_repeats() -> None:
    player = _player(
        {
            "strum": {"spacing": "1/8 beat"},
            "rhythm": {
                "kind": "pattern",
                "steps": [
                    {"kind": "hit", "duration": "1 beat", "repeats": 2},
                    {"kind": "tie", "duration": "1/2 beat"},
                ],
            },
        }
    )
    for key in [60, 64]:
        player.accept(0, decode_message([144, key, 90]))
    messages = player.advance(0)
    messages.extend(player.accept(25_000_000, decode_message([128, 64, 0])))
    messages.extend(player.accept(25_000_000, decode_message([144, 48, 80])))
    messages.extend(player.advance(700_000_000))
    assert [
        list(codecs.encode(m))[1] for m in messages if list(codecs.encode(m))[0] == 144
    ] == [60, 64, 60, 64]
    notes = player.take_events().notes
    assert max(n.at for n in notes if n.port == "note_end") > 1


@pytest.mark.parametrize("operation", ["stop", "clear", "seek"])
def test_cleanup_cancels_unsent_strums_in_every_lane(operation: str) -> None:
    player = _player(
        {
            "bank": {"kind": "latched"},
            "strum": {"spacing": "1/8 beat"},
            "rhythm": {"kind": "grid", "step": "1 beat"},
            "lanes": {
                "other": {
                    "channel": 2,
                    "bank": {"kind": "latched"},
                    "strum": {"spacing": "1/8 beat"},
                    "rhythm": {"kind": "grid", "step": "1 beat"},
                }
            },
        }
    )
    for key in [60, 64, 67]:
        player.accept(0, decode_message([144, key, 90]))
    player.advance(0)
    if operation == "stop":
        output = player.stop(25_000_000)
    elif operation == "clear":
        output = player.clear(25_000_000)
    else:
        output = player.accept(25_000_000, decode_message([242, 4, 0]))
    assert [list(codecs.encode(m)) for m in output] == [[128, 60, 0], [129, 60, 0]]
    if operation == "seek":
        player.stop(25_000_000)
    assert player.advance(400_000_000) == []
    assert not player.output_notes


def test_shared_expression_reaches_every_sounding_lane_channel() -> None:
    player = _player(
        {
            "strum": {"spacing": "1/8 beat"},
            "rhythm": {"kind": "grid", "step": "1 beat"},
            "lanes": {
                "other": {"channel": 3, "rhythm": {"kind": "grid", "step": "1 beat"}}
            },
        }
    )
    player.accept(0, decode_message([176, 2, 30]))
    for key in [60, 64]:
        player.accept(0, decode_message([144, key, 90]))
    assert [list(codecs.encode(m)) for m in player.advance(100_000_000)] == [
        [176, 2, 30],
        [144, 60, 90],
        [178, 2, 30],
        [146, 60, 90],
        [176, 2, 30],
        [144, 64, 90],
    ]
    assert [
        list(codecs.encode(m))
        for m in player.accept(110_000_000, decode_message([176, 2, 80]))
    ] == [[176, 2, 80], [178, 2, 80]]
    assert len(player.output_notes) == 3


def test_strum_rejects_incompatible_per_note_tuning_and_keeps_cleanup() -> None:
    player = _player(
        {
            "strum": {"spacing": "1/8 beat"},
            "tuning": {
                "source": {
                    "kind": "computed",
                    "notes_per_octave": 19,
                    "octave_ratio": "2",
                },
                "root_note": 69,
                "root_frequency": "440",
            },
            "bend_range": 2,
        }
    )
    for key in [69, 70]:
        player.accept(0, decode_message([144, key, 90]))
    player.advance(0)
    with pytest.raises(ValueError, match="same pitch bend"):
        player.advance(100_000_000)
    assert [list(codecs.encode(m)) for m in player.stop(100_000_000)] == [[128, 69, 0]]


def test_polyphonic_lifecycle_exhaustion_reserves_every_owned_release() -> None:
    player = _player(
        {
            "strum": {"spacing": "1/128 beat"},
            "rhythm": {"kind": "grid", "step": "1/64 beat"},
            "gate": 1000,
        }
    )
    for key in [60, 64, 67]:
        player.accept(0, decode_message([144, key, 90]))
    player.advance(10_000_000_000)
    batch = player.take_events()
    assert batch.exhausted
    player.stop(10_000_000_000)
    notes = batch.notes + player.take_events().notes
    assert sorted(n.occurrence for n in notes if n.port == "note_start") == sorted(
        n.occurrence for n in notes if n.port == "note_end"
    )
    assert not player.output_notes


def test_external_clock_is_observed_once_for_multiple_lanes() -> None:
    player = _player(
        {
            "lanes": {
                "other": {"channel": 2, "rhythm": {"kind": "grid", "step": "1/3 beat"}}
            }
        }
    )
    player.clock = TransportClock(mode=ClockMode.external)
    player.accept(0, decode_message([144, 60, 90]))
    for at, data in [(0, [250]), (0, [248]), (20_000_000, [248]), (40_000_000, [248])]:
        player.accept(at, decode_message(data))
    assert len(player.clock.observations) == 4
    player.advance(40_000_000)
    assert player.clock.beat == Fraction(1, 12)
    assert len(player.output_notes) == 2


def test_clear_leaves_held_lanes_playing_and_clears_latched_lanes() -> None:
    player = _player(
        {
            "lanes": {
                "latch": {
                    "channel": 2,
                    "bank": {"kind": "latched"},
                    "rhythm": {"kind": "grid", "step": "1/4 beat"},
                }
            }
        }
    )
    player.accept(0, decode_message([144, 60, 90]))
    player.advance(0)
    assert [list(codecs.encode(m)) for m in player.clear(25_000_000)] == [[129, 60, 0]]
    assert [n.channel for n in player.output_notes.values()] == [1]


def test_captured_bank_uses_its_configured_output_channel() -> None:
    player = _player(
        {
            "channel": 3,
            "bank": {"kind": "history"},
            "expression": {"source": "recorded", "timing": "fit", "gaps": "carry"},
        }
    )
    for at, data in [
        (0, [176, 2, 30]),
        (0, [144, 60, 81]),
        (50_000_000, [128, 60, 20]),
    ]:
        player.accept(at, decode_message(data))
    assert [list(codecs.encode(m)) for m in player.advance(225_000_000)] == [
        [178, 2, 30],
        [146, 60, 81],
        [130, 60, 20],
    ]
    batch = player.take_events()
    assert {n.channel for n in batch.notes} == {3}
    assert {e.channel for e in batch.events} == {3}


def test_strum_capacity_fails_before_any_attacks_are_queued() -> None:
    player = _player(
        {
            "strum": {"spacing": "1/128 beat"},
            "rhythm": {
                "kind": "pattern",
                "steps": [{"kind": "hit", "duration": "1 beat", "repeats": 5000}],
            },
        }
    )
    player.accept(0, decode_message([144, 60, 90]))
    with pytest.raises(ValueError, match="4096 pending-attack"):
        player.advance(0)
    assert player.stop(0) == []
    assert not player.output_notes


def _player(body: dict[str, object]) -> MidiPlayer:
    return MidiPlayer(
        profile=ArpeggiatorScore.model_validate(
            {
                "name": "strums-lanes",
                "title": "Strums and lanes",
                "body": {"rhythm": {"kind": "grid", "step": "1/4 beat"}} | body,
            }
        )
    )
