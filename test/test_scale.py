from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest
from meedee import codecs
from ufor.arpeggiator import Octaves, PitchRegister, ScaleTransform, Transposition
from ufor.scale import Scale

from arpeg.live import LiveArpeggiator
from arpeg.midi import MidiPlayer, decode_message
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/scale.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
def test_scale_traces_keep_source_identity_and_selector_progress(
    case: dict[str, object],
) -> None:
    profile = parse_profile(Path("conformance/up.toml").read_text())
    profile = profile.model_copy(
        update={
            "body": profile.body.model_copy(
                update={
                    "scale": ScaleTransform.model_validate(case["scale"]),
                    "register_limits": PitchRegister.model_validate(case["limits"])
                    if "limits" in case
                    else None,
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


def test_scale_history_preserves_recorded_expression_and_release_velocity() -> None:
    profile = parse_profile(Path("conformance/history-wind.toml").read_text())
    profile = profile.model_copy(
        update={
            "body": profile.body.model_copy(
                update={
                    "scale": ScaleTransform(
                        definition=Scale(notes="CDEFGAB"), degrees=1
                    )
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
        [144, 62, 100],
        [176, 2, 80],
        [128, 62, 20],
    ]


def test_repeats_keep_the_mapped_pitch_after_a_bank_edit() -> None:
    profile = parse_profile("""name="scale"
title="Scale"
[body]
retrigger="bank_edit"
scale={definition={notes="CDEFGAB"},degrees=1}
rhythm={kind="pattern",steps=[{kind="hit",duration="1 beat",repeats=3}]}
""")
    arp = LiveArpeggiator(profile=profile)
    arp.note_on(Fraction(0), 61, 100)
    assert [e.key for e in arp.advance(Fraction(0)) if e.kind == "on"] == [62]
    arp.note_on(Fraction(1, 6), 64, 90)
    assert [e.key for e in arp.advance(Fraction(2, 3)) if e.kind == "on"] == [62, 62]


def test_ties_sustain_the_mapped_pitch_until_their_end() -> None:
    profile = parse_profile("""name="scale"
title="Scale"
[body]
scale={definition={notes="CDEFGAB"},degrees=1}
[body.rhythm]
kind="pattern"
steps=[{kind="hit",duration="1 beat"},{kind="tie",duration="1 beat"}]
""")
    arp = LiveArpeggiator(profile=profile)
    arp.note_on(Fraction(0), 61, 100)
    assert [(e.kind, e.key) for e in arp.advance(Fraction(0))] == [("on", 62)]
    assert arp.advance(Fraction(1)) == []
    assert [(e.kind, e.key) for e in arp.stop(Fraction(3, 2))] == [("off", 62)]
