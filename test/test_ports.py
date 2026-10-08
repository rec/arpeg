from fractions import Fraction
from pathlib import Path
from tomllib import loads

import mido
import pytest
from ufor import arpeggiator_ports, motion

from arpeg.midi import MidiPlayer
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/ports.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
@pytest.mark.parametrize("poll_us", [None, 1000])
def test_shared_motion_port_trace(case: dict[str, object], poll_us: int | None) -> None:
    player = MidiPlayer(
        profile=parse_profile(Path(f"conformance/{case['profile']}.toml").read_text())
    )
    actions = case["actions"]
    assert isinstance(actions, list)
    previous_at = 0
    for action in actions:
        at = action["at"] * 1000
        output: list[mido.Message] = []
        events: list[arpeggiator_ports.ArpeggiatorOutput] = []
        if poll_us is not None:
            for tick in range(previous_at + poll_us * 1000, at, poll_us * 1000):
                output.extend(player.advance(tick))
                batch = player.take_events()
                assert not batch.exhausted
                events.extend(batch.events)
        if "data" in action:
            output.extend(player.accept(at, mido.Message.from_bytes(action["data"])))
        elif "capture" in action:
            output.extend(player.capture(at, action["capture"]))
        elif "port" in action:
            output.extend(
                player.control(
                    at,
                    arpeggiator_ports.ArpeggiatorControl.model_validate(
                        {"port": action["port"], "value": action["value"]}
                    ),
                )
            )
        else:
            output.extend(player.advance(at))
        batch = player.take_events()
        assert not batch.exhausted
        events.extend(batch.events)
        assert [m.bytes() for m in output] == action["output"], action
        assert [
            [str(e.at), e.port.value, e.index, e.revision] for e in events
        ] == action["events"], action
        player = MidiPlayer.model_validate_json(player.model_dump_json())
        previous_at = at


def test_invalid_controls_leave_pending_music_unchanged() -> None:
    player = MidiPlayer(profile=parse_profile(Path("conformance/up.toml").read_text()))
    player.accept(0, mido.Message.from_bytes([144, 60, 100]))
    player.advance(0)
    snapshot = player.model_dump_json()
    with pytest.raises(ValueError, match="explicit seed"):
        player.control(
            50_000_000,
            arpeggiator_ports.ArpeggiatorControl(
                port=arpeggiator_ports.ArpeggiatorInputPort.density, value="1/2"
            ),
        )
    assert player.model_dump_json() == snapshot
    assert [m.bytes() for m in player.advance(100_000_000)] == [[128, 60, 0]]


def test_full_event_buffer_skips_attacks_and_still_releases_owned_notes() -> None:
    player = MidiPlayer(profile=parse_profile(Path("conformance/up.toml").read_text()))
    player.accept(0, mido.Message.from_bytes([144, 60, 100]))
    output = player.advance(256_000_000_000)
    assert sum(m.type == "note_on" for m in output) == 2048
    assert sum(m.type == "note_off" for m in output) == 2048
    batch = player.take_events()
    assert len(batch.events) == 4096
    assert batch.exhausted
    assert [m.bytes() for m in player.advance(256_125_000_000)] == [[144, 60, 100]]
    assert not player.take_events().exhausted


def test_motion_samples_drive_density_and_receive_hit_events() -> None:
    source = motion.MotionUse.model_validate(
        {
            "clock": "beats",
            "body": {
                "kind": "cycle",
                "shape": "square",
                "rate": "1",
                "center": 0.5,
                "depth": 0.5,
            },
        }
    )
    state = motion.initial_motion(source, Fraction(0))
    receiver = motion.MotionUse.model_validate(
        {
            "clock": "beats",
            "body": {
                "kind": "contour",
                "start": "event",
                "retrigger": "reset",
                "segments": [{"duration": "1/4 beat", "to": 1.0}],
            },
        }
    )
    receiver_state = motion.initial_motion(receiver, Fraction(0))
    player = MidiPlayer(profile=parse_profile(Path("conformance/up.toml").read_text()))
    player.accept(0, mido.Message.from_bytes([144, 60, 100]))
    hits: list[int] = []
    for index in range(5):
        at = Fraction(index, 4)
        value = motion.motion_at(source, state, at).value
        player.control(
            index * 125_000_000,
            arpeggiator_ports.ArpeggiatorControl(
                port=arpeggiator_ports.ArpeggiatorInputPort.density,
                value=Fraction(str(value)),
            ),
        )
        player.advance(index * 125_000_000)
        for event in player.take_events().events:
            if event.port == arpeggiator_ports.ArpeggiatorOutputPort.hit:
                hits.append(event.index)
                receiver_state = motion.motion_event(
                    receiver,
                    receiver_state,
                    motion.MotionEvent(
                        at=event.at, ordinal=event.index, action="start"
                    ),
                )
    assert hits == [0, 1, 4]
    assert motion.motion_at(receiver, receiver_state, Fraction(9, 8)).value == 0.5
