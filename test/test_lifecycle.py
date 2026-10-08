from pathlib import Path
from tomllib import loads

import mido
import pytest
from ufor.arpeggiator_ports import ArpeggiatorNoteEvent

from arpeg.midi import MidiPlayer
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/lifecycle.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
@pytest.mark.parametrize("poll_us", [None, 1000])
def test_lifecycle_matches_delivered_midi_and_survives_snapshots(
    case: dict[str, object], poll_us: int | None
) -> None:
    player = MidiPlayer(
        profile=parse_profile(Path(f"conformance/{case['profile']}.toml").read_text())
    )
    previous = 0
    actions = case["actions"]
    assert isinstance(actions, list)
    for action in actions:
        at = action["at"] * 1000
        messages: list[mido.Message] = []
        notes: list[ArpeggiatorNoteEvent] = []
        if poll_us is not None:
            for tick in range(previous + poll_us * 1000, at, poll_us * 1000):
                messages.extend(player.advance(tick))
                batch = player.take_events()
                assert not batch.exhausted
                notes.extend(batch.notes)
        if "data" in action:
            messages.extend(player.accept(at, mido.Message.from_bytes(action["data"])))
        elif "capture" in action:
            messages.extend(player.capture(at, action["capture"]))
        elif "stop" in action:
            messages.extend(player.stop(at))
        elif "clear" in action:
            messages.extend(player.clear(at))
        else:
            messages.extend(player.advance(at))
        batch = player.take_events()
        assert not batch.exhausted
        notes.extend(batch.notes)
        assert [m.bytes() for m in messages] == action["output"], action
        assert [
            [str(n.at), n.port.value, n.occurrence, n.source, n.key, n.velocity]
            for n in notes
        ] == action["notes"], action
        player = MidiPlayer.model_validate_json(player.model_dump_json())
        previous = at


def test_full_note_buffer_skips_repeats_without_losing_ends_or_replaying_attacks() -> (
    None
):
    player = MidiPlayer(
        profile=parse_profile(Path("conformance/lifecycle-repeats.toml").read_text())
    )
    player.accept(0, mido.Message.from_bytes([144, 60, 100]))
    output = player.advance(125_000_000_000)
    batch = player.take_events()
    assert batch.exhausted
    assert len(batch.notes) == len(output) == 4096
    assert [n.occurrence for n in batch.notes[::2]] == list(range(2048))
    assert all(
        a.occurrence == b.occurrence and a.port == "note_start" and b.port == "note_end"
        for a, b in zip(batch.notes[::2], batch.notes[1::2], strict=True)
    )
    assert [m.bytes() for m in player.advance(125_050_000_000)] == [
        [144, 60, 100],
        [128, 60, 0],
    ]
    assert [n.occurrence for n in player.take_events().notes] == [2048, 2048]


def test_owned_end_keeps_its_reserved_slot_across_an_undrained_snapshot() -> None:
    player = MidiPlayer(
        profile=parse_profile(Path("conformance/live-wind.toml").read_text())
    )
    player.accept(0, mido.Message.from_bytes([144, 60, 100]))
    player.advance(255_875_000_000)
    player = MidiPlayer.model_validate_json(player.model_dump_json())
    assert [m.bytes() for m in player.stop(255_876_000_000)] == [[128, 60, 0]]
    batch = player.take_events()
    assert not batch.exhausted
    assert len(batch.notes) == 4096
    assert batch.notes[-1].port == "note_end"
    assert batch.notes[-1].occurrence == 2047
