from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest
from meedee import codecs
from meedee.messages import NoteOn
from ufor.arpeggiator import Octaves, PitchRegister, Transposition

from arpeg.live import LiveArpeggiator
from arpeg.midi import MidiPlayer, decode_message
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/register.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
def test_register_traces_keep_source_identity_and_selector_progress(
    case: dict[str, object],
) -> None:
    profile = parse_profile(Path("conformance/up.toml").read_text())
    profile = profile.model_copy(
        update={
            "body": profile.body.model_copy(
                update={
                    "register_limits": PitchRegister.model_validate(case["limits"]),
                    "octaves": Octaves.model_validate(
                        {"offsets": case.get("octaves", [0])}
                    ),
                    "transposition": Transposition.model_validate(
                        {
                            "semitones": case.get("semitones", 0),
                            "boundary": case.get("midi_boundary", "drop"),
                        }
                    ),
                }
            )
        }
    )
    arp = LiveArpeggiator(profile=profile)
    notes = case["notes"]
    keys = case["keys"]
    assert isinstance(notes, list) and isinstance(keys, list)
    for n in notes:
        arp.note_on(Fraction(0), *n)
    actual: list[int] = []
    for i in range(len(keys)):
        attacks = [e for e in arp.advance(Fraction(i, 4)) if e.kind == "on"]
        actual.append(attacks[0].key if attacks else -1)
        if attacks:
            assert attacks[0].velocity == notes[attacks[0].source_id][1]
        arp = LiveArpeggiator.model_validate_json(arp.model_dump_json())
    assert actual == keys


def test_folded_history_preserves_recorded_expression_and_release_velocity() -> None:
    profile = parse_profile(Path("conformance/history-wind.toml").read_text())
    profile = profile.model_copy(
        update={
            "body": profile.body.model_copy(
                update={
                    "register_limits": PitchRegister(low=72, high=84, boundary="fold")
                }
            )
        }
    )
    player = MidiPlayer(profile=profile)
    for at, data in [
        (0, [176, 2, 0]),
        (0, [144, 60, 100]),
        (8_000_000, [176, 2, 80]),
        (50_000_000, [128, 60, 20]),
    ]:
        player.accept(at, decode_message(data))
    output = player.advance(225_000_000)
    assert [list(codecs.encode(m)) for m in output] == [
        [176, 2, 0],
        [144, 72, 100],
        [176, 2, 80],
        [128, 72, 20],
    ]


def test_register_error_preserves_an_owned_note_for_cleanup() -> None:
    profile = parse_profile("""name="register"
title="Register"
[body]
rhythm={kind="grid",step="1/4 beat"}
gate=2
register_limits={low=60,high=72,boundary="error"}
""")
    player = MidiPlayer(profile=profile)
    player.accept(0, NoteOn(channel=1, note=60, velocity=100))
    player.accept(0, NoteOn(channel=1, note=73, velocity=90))
    assert [list(codecs.encode(m)) for m in player.advance(0)] == [[144, 60, 100]]
    with pytest.raises(RuntimeError, match="register limits"):
        player.advance(125_000_000)
    assert [list(codecs.encode(m)) for m in player.stop(125_000_000)] == [[128, 60, 0]]
