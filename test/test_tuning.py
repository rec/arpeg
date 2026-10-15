from math import log2
from pathlib import Path
from tomllib import loads

import mido
import pytest
from ufor.arpeggiator import Arpeggiator, ArpeggiatorScore
from ufor.tuning import Tuning

from arpeg.midi import MidiPlayer
from arpeg.output import MidiDelivery
from arpeg.profile import parse_profile
from arpeg.tuning import PreparedTuning


def test_nineteen_divisions_encode_tuning_and_current_expression() -> None:
    player = MidiPlayer(
        profile=ArpeggiatorScore(
            name="tuning",
            title="Tuning",
            body=Arpeggiator.model_validate(
                {
                    "rhythm": {"kind": "grid", "step": "1 beat"},
                    "tuning": {"source": {"kind": "computed", "notes_per_octave": 19}},
                    "bend_range": 2,
                }
            ),
        )
    )
    player.accept(0, mido.Message("note_on", note=70, velocity=100))
    assert [m.bytes() for m in player.advance(0)] == [[224, 27, 52], [144, 70, 100]]
    assert [
        m.bytes()
        for m in player.accept(1_000_000, mido.Message("pitchwheel", pitch=4096))
    ] == [[224, 27, 84]]
    player = MidiPlayer.model_validate_json(player.model_dump_json())
    assert [m.bytes() for m in player.stop(2_000_000)] == [[128, 70, 0]]


@pytest.mark.parametrize(
    "source",
    [
        {"kind": "computed", "notes_per_octave": 19},
        {"kind": "computed", "notes_per_octave": 12, "denominator_limit": 16},
        {"kind": "ratios", "values": ["1", "9/8", "5/4"], "repeat_ratio": "2"},
        {"kind": "intervals", "intervals": ["9/8", "10/9", "8/5"]},
        {
            "kind": "frequencies",
            "first_note": 69,
            "values": ["440", "440/(2^(1/12))", "660"],
        },
    ],
)
def test_prepared_pitches_match_ufor_frequency_semantics(
    source: dict[str, object],
) -> None:
    tuning = Tuning.model_validate({"source": source})
    body = Arpeggiator.model_validate(
        {
            "rhythm": {"kind": "grid", "step": "1 beat"},
            "tuning": tuning,
            "bend_range": 2,
        }
    )
    prepared = PreparedTuning.prepare(body)
    assert prepared is not None
    indices = [69, 70, 71] if source["kind"] == "frequencies" else [60, 69, 70, 81]
    for i in indices:
        assert prepared.pitch(i) == pytest.approx(
            69 + 12 * log2(float(tuning(i)) / 440), abs=1e-10
        )


@pytest.mark.parametrize(
    "transform", [{"octaves": {"offsets": [1]}}, {"inversion": 1}, {"voicing": [1]}]
)
def test_period_transforms_follow_a_three_to_one_tuning(
    transform: dict[str, object],
) -> None:
    body = Arpeggiator.model_validate(
        {
            "rhythm": {"kind": "grid", "step": "1 beat"},
            "tuning": {
                "source": {
                    "kind": "computed",
                    "notes_per_octave": 13,
                    "octave_ratio": "3",
                }
            },
            "bend_range": 2,
            **transform,
        }
    )
    player = MidiPlayer(
        profile=ArpeggiatorScore(name="tritave", title="Tritave", body=body)
    )
    player.accept(0, mido.Message("note_on", note=69, velocity=100))
    assert [m.bytes() for m in player.advance(0)] == [[224, 80, 64], [144, 88, 100]]


def test_bend_overflow_cleanup_releases_the_note_actually_sent() -> None:
    body = Arpeggiator.model_validate(
        {
            "rhythm": {"kind": "grid", "step": "1/4 beat"},
            "tuning": {"source": {"kind": "computed", "notes_per_octave": 19}},
            "bend_range": 2,
        }
    )
    player = MidiPlayer(
        profile=ArpeggiatorScore(name="overflow", title="Overflow", body=body)
    )
    for k in [69, 70]:
        player.accept(0, mido.Message("note_on", note=k, velocity=100))
    delivery = MidiDelivery()
    sent: list[mido.Message] = []
    delivery.send(player.advance(0), 0, sent.append)
    delivery.send(
        player.accept(1_000_000, mido.Message("pitchwheel", pitch=-8192)),
        1_000_000,
        sent.append,
    )
    with pytest.raises(RuntimeError, match="bend range"):
        player.advance(125_000_000)
    delivery.cleanup(player.stop(125_000_000), sent.append)
    assert sent[-1].bytes() == [128, 69, 0]
    assert not delivery.owned


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/tuning.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
def test_shared_tuning_wire_traces(case: dict[str, object]) -> None:
    settings = {"rhythm": {"kind": "grid", "step": "1/4 beat"}}
    body = case["body"]
    assert isinstance(body, dict)
    settings.update(body)
    player = MidiPlayer(
        profile=ArpeggiatorScore(
            name="tuning", title="Tuning", body=Arpeggiator.model_validate(settings)
        )
    )
    inputs = case["input"]
    assert isinstance(inputs, list)
    for data in inputs:
        assert player.accept(0, mido.Message.from_bytes(data)) == []
    assert [m.bytes() for m in player.advance(0)] == case["output"]


def test_recorded_bends_keep_the_tuning_offset_and_release_velocity() -> None:
    profile = parse_profile(
        Path("conformance/history-wind.toml")
        .read_text()
        .replace(
            "[body]",
            '[body]\nbend_range=2\ntuning={source={kind="computed",notes_per_octave=19}}',
        )
    )
    player = MidiPlayer(profile=profile)
    for at, data in [
        (0, [224, 0, 64]),
        (0, [144, 70, 100]),
        (8_000_000, [224, 0, 96]),
        (50_000_000, [128, 70, 20]),
    ]:
        player.accept(at, mido.Message.from_bytes(data))
    output = player.advance(225_000_000)
    assert [m.bytes() for m in output] == [
        [224, 27, 52],
        [224, 27, 52],
        [144, 70, 100],
        [224, 27, 84],
        [128, 70, 20],
    ]
