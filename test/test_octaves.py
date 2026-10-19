from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest
from meedee import codecs
from meedee.messages import NoteOn
from ufor.arpeggiator import Octaves, Shuffle

from arpeg.live import LiveArpeggiator
from arpeg.midi import MidiPlayer, decode_message
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/octaves.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
def test_octave_traces_preserve_source_identity_velocity_and_snapshot(
    case: dict[str, object],
) -> None:
    profile = parse_profile(str(case["profile"]))
    arp = LiveArpeggiator(profile=profile)
    notes = case["notes"]
    assert isinstance(notes, list)
    for n in notes:
        arp.note_on(Fraction(0), *n)
    keys = case["keys"]
    assert isinstance(keys, list)
    actual = []
    for i in range(len(keys)):
        for event in arp.advance(Fraction(i, 4)):
            if event.kind == "on":
                actual.append(event.key)
                assert event.velocity == notes[event.source_id][1]
        arp = LiveArpeggiator.model_validate_json(arp.model_dump_json())
    assert actual == keys


@pytest.mark.parametrize(
    "boundary, expected", [("drop", [120, 120]), ("fold", [120, 120, 120])]
)
def test_octave_pitch_limits_keep_traversal_moving(
    boundary: str, expected: list[int]
) -> None:
    profile = parse_profile(
        f'''name="octaves"
title="Octaves"
[body]
rhythm={{kind="grid",step="1/4 beat"}}
octaves={{offsets=[0,1]}}
transposition={{boundary="{boundary}"}}
'''
    )
    arp = LiveArpeggiator(profile=profile)
    arp.note_on(Fraction(0), 120, 100)
    assert [e.key for e in arp.advance(Fraction(1, 2)) if e.kind == "on"] == expected


def test_octave_history_replays_original_expression_at_each_pitch() -> None:
    profile = parse_profile(Path("conformance/history-wind.toml").read_text())
    profile = profile.model_copy(
        update={
            "body": profile.body.model_copy(
                update={"octaves": Octaves(offsets=[-1, 0, 1])}
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
    output = [list(codecs.encode(m)) for m in player.advance(475_000_000)]
    for i, key in enumerate([48, 60, 72]):
        assert output[i * 4 : (i + 1) * 4] == [
            [176, 2, 0],
            [144, key, 100],
            [176, 2, 80],
            [128, key, 20],
        ]


def test_shuffled_octave_entries_survive_snapshot_and_source_removal() -> None:
    profile = parse_profile(Path("conformance/shuffle.toml").read_text())
    profile = profile.model_copy(
        update={
            "body": profile.body.model_copy(
                update={
                    "octaves": Octaves(offsets=[0, 1]),
                    "selection": Shuffle(on_edit="preserve", no_repeat=True),
                }
            )
        }
    )
    arp = LiveArpeggiator(profile=profile)
    arp.note_on(Fraction(0), 60, 100)
    arp.note_on(Fraction(0), 64, 90)
    first = [e.key for e in arp.advance(Fraction(3, 4)) if e.kind == "on"]
    assert sorted(first) == [60, 64, 72, 76]
    arp = LiveArpeggiator.model_validate_json(arp.model_dump_json())
    arp.note_off(Fraction(1), 60)
    after = [e.key for e in arp.advance(Fraction(7, 4)) if e.kind == "on"]
    assert after[0] != first[-1]
    assert sorted(after[:2]) == [64, 76]


def test_repeats_keep_the_selected_octave_after_a_bank_edit() -> None:
    profile = parse_profile("""name="octaves"
title="Octaves"
[body]
retrigger="bank_edit"
octaves={offsets=[1,0]}
rhythm={kind="pattern",steps=[{kind="hit",duration="1 beat",repeats=3}]}
""")
    arp = LiveArpeggiator(profile=profile)
    arp.note_on(Fraction(0), 60, 100)
    assert [e.key for e in arp.advance(Fraction(0)) if e.kind == "on"] == [72]
    arp.note_on(Fraction(1, 6), 64, 90)
    assert [e.key for e in arp.advance(Fraction(2, 3)) if e.kind == "on"] == [72, 72]


def test_octave_range_errors_allow_owned_note_cleanup() -> None:
    profile = parse_profile("""name="octaves"
title="Octaves"
[body]
octaves={offsets=[0,1]}
rhythm={kind="grid",step="1/4 beat"}
transposition={boundary="error"}
gate=2
""")
    player = MidiPlayer(profile=profile)
    player.accept(0, NoteOn(channel=1, note=120, velocity=100))
    assert [list(codecs.encode(m)) for m in player.advance(0)] == [[144, 120, 100]]
    with pytest.raises(ValueError, match="outside MIDI"):
        player.advance(125_000_000)
    assert [list(codecs.encode(m)) for m in player.stop(125_000_000)] == [[128, 120, 0]]
