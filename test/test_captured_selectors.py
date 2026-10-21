from pathlib import Path
from tomllib import loads

import pytest
from meedee import codecs
from ufor.arpeggiator import ArpeggiatorScore

from arpeg.midi import MidiPlayer, decode_message
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/captured-selectors.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
@pytest.mark.parametrize("bank", ["history", "phrase"])
@pytest.mark.parametrize("polling", [False, True])
def test_captured_selectors_preserve_gestures_and_match_shared_traces(
    case: dict[str, object], bank: str, polling: bool
) -> None:
    profile = parse_profile(Path(f"conformance/{bank}-wind.toml").read_text())
    profile = ArpeggiatorScore.model_validate(
        profile.model_dump()
        | {
            "body": profile.body.model_dump()
            | {
                "selection": case["selection"],
                "seed": 42,
                "octaves": case.get("octaves", {"offsets": [0]}),
                "patterns": case.get("patterns"),
                "retrigger": "on_empty",
            }
        }
    )
    player = MidiPlayer(profile=profile)
    if bank == "phrase":
        player.capture(0, "record")
    for i, key in enumerate([60, 64, 67]):
        at = i * 20_000_000
        player.accept(at, decode_message([176, 2, (i + 1) * 10]))
        player.accept(at + 1_000_000, decode_message([144, key, 90 - i * 10]))
        player.accept(at + 10_000_000, decode_message([128, key, (i + 1) * 11]))
    if bank == "phrase":
        player.capture(60_000_000, "commit")
    player.take_events()
    messages = []
    notes = []
    for at in (
        [*range(60_000_000, 600_000_000, 17_000_000), 600_000_000]
        if polling
        else [600_000_000]
    ):
        messages.extend(list(codecs.encode(m)) for m in player.advance(at))
        notes.extend(player.take_events().notes)
        player = MidiPlayer.model_validate_json(player.model_dump_json())
    starts = [n for n in notes if n.port == "note_start"]
    ends = [n for n in notes if n.port == "note_end"]
    assert [n.key for n in starts] == case["keys"]
    assert [str(n.at) for n in starts] == case["beats"]
    assert [(n.occurrence, n.source) for n in starts] == [
        (n.occurrence, n.source) for n in ends
    ]
    expected = {60: (90, 10, 11), 64: (80, 20, 22), 67: (70, 30, 33), 72: (90, 10, 11)}
    assert [n.velocity for n in starts] == [expected[n.key][0] for n in starts]
    assert [n.velocity for n in ends] == [expected[n.key][2] for n in starts]
    for i, message in enumerate(messages):
        if message[0] == 144:
            assert messages[i - 1] == [176, 2, expected[message[1]][1]]
