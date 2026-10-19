from fractions import Fraction
from pathlib import Path
from tomllib import loads

import pytest
from meedee import codecs
from ufor.arpeggiator_ports import ArpeggiatorControl

from arpeg.clock import ClockMode, TransportClock
from arpeg.midi import MidiPlayer, decode_message
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/pre-note-expression.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
def test_expression_precedes_attacks_in_the_shared_wire_trace(
    case: dict[str, object],
) -> None:
    profile = case["profile"]
    actions = case["actions"]
    assert isinstance(profile, str) and isinstance(actions, list)
    player = _player(profile)
    for action in actions:
        tick = action["at_us"] * 1000
        output = []
        for data in action.get("data", []):
            output.extend(player.accept(tick, decode_message(data)))
        if "capture" in action:
            output.extend(player.capture(tick, action["capture"]))
        if action.get("stop"):
            output.extend(player.stop(tick))
        if action.get("schedule"):
            player.schedule(tick, tempo=action.get("tempo"))
            player = MidiPlayer.model_validate_json(player.model_dump_json())
            assert not player.take_events().exhausted
            output.extend(
                player.realize(
                    [
                        ArpeggiatorControl(port=p, value=Fraction(v))
                        for p, v in action.get("controls", [])
                    ]
                )
            )
        assert [list(codecs.encode(m)) for m in output] == action["output"], action
        player = MidiPlayer.model_validate_json(player.model_dump_json())


def test_hits_precede_midi_but_note_lifecycle_follows_realization_once() -> None:
    player = _player()
    player.accept(0, decode_message([144, 60, 100]))
    player.schedule(0)
    batch = player.take_events()
    assert [e.port for e in batch.events][-1] == "hit"
    assert batch.notes == []
    assert [
        list(codecs.encode(m))
        for m in player.realize([ArpeggiatorControl(port="breath", value=1)])
    ] == [[176, 2, 127], [144, 60, 100]]
    batch = player.take_events()
    assert batch.events == []
    assert [n.port for n in batch.notes] == ["note_start"]
    with pytest.raises(ValueError, match="schedule MIDI"):
        player.realize()
    assert player.advance(0) == []
    assert player.take_events().notes == []


def test_pending_phase_rejects_mutation_and_invalid_controls_without_losing_music() -> (
    None
):
    player = _player("envelfo-phrase")
    player.accept(0, decode_message([144, 60, 100]))
    player.schedule(0)
    before = player.model_dump_json()
    with pytest.raises(ValueError, match="realize scheduled MIDI"):
        player.accept(0, decode_message([144, 64, 90]))
    with pytest.raises(ValueError, match="realize scheduled MIDI"):
        player.schedule(0)
    with pytest.raises(ValueError, match="realize scheduled MIDI"):
        player.advance(0)
    with pytest.raises(ValueError, match="realize scheduled MIDI"):
        player.set_tempo(0, 60)
    with pytest.raises(ValueError, match="realize scheduled MIDI"):
        player.clear(0)
    with pytest.raises(ValueError, match="realize scheduled MIDI"):
        player.capture(0, "record")
    with pytest.raises(ValueError, match="realize scheduled MIDI"):
        player.control(0, ArpeggiatorControl(port="breath", value=1))
    with pytest.raises(ValueError, match="only expression"):
        player.realize([ArpeggiatorControl(port="gate", value=1)])
    with pytest.raises(ValueError, match="not owned"):
        player.realize(
            [
                ArpeggiatorControl(port="breath", value=1),
                ArpeggiatorControl(port="bend", value=0),
            ]
        )
    assert player.model_dump_json() == before
    assert player.realize([ArpeggiatorControl(port="breath", value=1)]) == []


def test_expression_cannot_span_multiple_attack_boundaries() -> None:
    player = _player()
    player.accept(0, decode_message([144, 60, 100]))
    player.schedule(125_000_000)
    before = player.model_dump_json()
    with pytest.raises(ValueError, match="each attack boundary"):
        player.realize([ArpeggiatorControl(port="breath", value=1)])
    assert player.model_dump_json() == before
    assert [list(codecs.encode(m)) for m in player.realize()] == [
        [144, 60, 100],
        [128, 60, 0],
        [144, 60, 100],
    ]


@pytest.mark.parametrize("sounding", [False, True])
@pytest.mark.parametrize("profile", ["envelfo-expression", "envelfo-phrase"])
def test_stop_cancels_unsent_attacks_and_releases_only_the_actual_note(
    sounding: bool,
    profile: str,
) -> None:
    player = _player(profile)
    if profile == "envelfo-phrase":
        player.capture(0, "record")
        for tick, data in [
            (0, [144, 60, 100]),
            (10_000_000, [128, 60, 20]),
            (10_000_000, [144, 64, 90]),
            (20_000_000, [128, 64, 20]),
        ]:
            player.accept(tick, decode_message(data))
        player.capture(20_000_000, "commit")
        first, next_tick = 125_000_000, 250_000_000
    else:
        player.accept(0, decode_message([144, 60, 100]))
        player.accept(0, decode_message([144, 64, 90]))
        first, next_tick = 0, 125_000_000
    if sounding:
        player.advance(first)
        player.take_events()
    tick = next_tick if sounding else first
    player.schedule(tick)
    assert [list(codecs.encode(m)) for m in player.stop(tick)] == (
        [[128, 60, 0]] if sounding else []
    )
    notes = player.take_events().notes
    assert [(n.port, n.key) for n in notes] == ([("note_end", 60)] if sounding else [])
    assert player.advance(500_000_000) == []


def test_fractional_attack_boundaries_survive_host_clock_rounding() -> None:
    text = (
        Path("conformance/envelfo-expression.toml")
        .read_text()
        .replace("1/4 beat", "1/7 beat")
    )
    player = MidiPlayer(profile=parse_profile(text))
    player.accept(0, decode_message([144, 60, 100]))
    for i, tick in enumerate([0, 71_429_000, 142_858_000]):
        player.schedule(tick)
        batch = player.take_events()
        assert [e.at for e in batch.events if e.port == "hit"] == [Fraction(i, 7)]
        output = player.realize(
            [ArpeggiatorControl(port="breath", value=Fraction(i + 1, 4))]
        )
        expected = ([[128, 60, 0]] if i else []) + [
            [176, 2, [32, 64, 95][i]],
            [144, 60, 100],
        ]
        assert [list(codecs.encode(m)) for m in output] == expected


def test_external_clock_timeout_releases_before_discarding_new_expression() -> None:
    player = _player()
    player.clock = TransportClock(mode=ClockMode.external)
    for data in [[250], [248], [144, 60, 100]]:
        player.accept(0, decode_message(data))
    player.schedule(0)
    assert [
        list(codecs.encode(m))
        for m in player.realize([ArpeggiatorControl(port="breath", value=1)])
    ] == [[176, 2, 127], [144, 60, 100]]
    player.schedule(500_000_000)
    assert [
        list(codecs.encode(m))
        for m in player.realize([ArpeggiatorControl(port="breath", value=0)])
    ] == [[128, 60, 0]]


def _player(name: str = "envelfo-expression") -> MidiPlayer:
    return MidiPlayer(
        profile=parse_profile(Path(f"conformance/{name}.toml").read_text())
    )
