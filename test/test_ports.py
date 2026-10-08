from fractions import Fraction
from pathlib import Path
from tomllib import loads

import mido
import pytest
from ufor import arpeggiator_ports, motion
from ufor.arpeggiator import SelectionOffset, Transposition

from arpeg.live import LiveArpeggiator
from arpeg.midi import MidiPlayer
from arpeg.ports import PerformancePorts
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "name",
    [
        "up",
        "weighted-walk",
        "shuffle",
        "choice",
        "alternating",
        "inside-out",
        "outside-in",
        "index-pattern",
        "live-latch",
    ],
)
def test_offset_keeps_each_selectors_progression_and_emits_target_identity(
    name: str,
) -> None:
    profile = parse_profile(Path(f"conformance/{name}.toml").read_text())
    baseline = LiveArpeggiator(profile=profile)
    shifted = LiveArpeggiator(
        profile=profile.model_copy(
            update={
                "body": profile.body.model_copy(
                    update={"selection_offset": SelectionOffset(ranks=1)}
                )
            }
        )
    )
    keys = [67, 60, 64]
    for i, key in enumerate(keys):
        baseline.note_on(Fraction(0), key, 100 - i)
        shifted.note_on(Fraction(0), key, 100 - i)
    expected = [
        (n.source_id + 1) % 3 for n in baseline.advance(Fraction(2)) if n.kind == "on"
    ]
    actual = [n for n in shifted.advance(Fraction(2)) if n.kind == "on"]
    assert [(n.source_id, n.key, n.velocity) for n in actual] == [
        (i, keys[i], 100 - i) for i in expected
    ]
    shifted = LiveArpeggiator.model_validate_json(shifted.model_dump_json())
    shifted.control(
        Fraction(9, 4),
        arpeggiator_ports.ArpeggiatorControl(port="selection_offset", value=0),
    )
    assert [
        (n.source_id, n.key) for n in shifted.advance(Fraction(3)) if n.kind == "on"
    ] == [(n.source_id, n.key) for n in baseline.advance(Fraction(3)) if n.kind == "on"]


def test_offset_breaks_equal_pitch_ties_by_source_identity_and_keeps_repeats() -> None:
    profile = parse_profile(Path("conformance/custom-steps.toml").read_text())
    engine = LiveArpeggiator(profile=profile)
    engine.note_on(Fraction(0), 60, 100)
    engine.note_on(Fraction(0), 60, 90)
    engine.control(
        Fraction(0),
        arpeggiator_ports.ArpeggiatorControl(port="selection_offset", value=1),
    )
    assert [
        (n.source_id, n.velocity)
        for n in engine.advance(Fraction(5, 8))
        if n.kind == "on"
    ] == [(1, 90), (0, 100)]
    engine.control(
        Fraction(2, 3),
        arpeggiator_ports.ArpeggiatorControl(port="selection_offset", value=0),
    )
    assert [n.source_id for n in engine.advance(Fraction(9, 8)) if n.kind == "on"] == [
        0,
        0,
        0,
    ]


def test_transposition_keeps_pending_repeats_and_source_identity() -> None:
    engine = LiveArpeggiator(
        profile=parse_profile(Path("conformance/custom-steps.toml").read_text())
    )
    engine.note_on(Fraction(0), 60, 100)
    engine.advance(Fraction(5, 8))
    engine.control(
        Fraction(2, 3),
        arpeggiator_ports.ArpeggiatorControl(port="transposition", value=12),
    )
    events = engine.advance(Fraction(9, 8))
    assert [(e.key, e.source_id) for e in events if e.kind == "on"] == [
        (60, 0),
        (60, 0),
        (72, 0),
    ]
    assert engine.input[0].key == 60


@pytest.mark.parametrize("profile_name", ["up", "phrase-wind"])
def test_pitch_range_error_still_releases_the_last_delivered_note(
    profile_name: str,
) -> None:
    profile = parse_profile(Path(f"conformance/{profile_name}.toml").read_text())
    profile = profile.model_copy(
        update={
            "body": profile.body.model_copy(
                update={"transposition": Transposition(boundary="error")}
            )
        }
    )
    player = MidiPlayer(profile=profile)
    if profile_name == "phrase-wind":
        player.capture(0, "record")
    player.accept(0, mido.Message.from_bytes([144, 120, 100]))
    if profile_name == "phrase-wind":
        player.capture(0, "commit")
    assert [m.bytes() for m in player.advance(0)] == [[144, 120, 100]]
    player.control(
        50_000_000, arpeggiator_ports.ArpeggiatorControl(port="transposition", value=12)
    )
    with pytest.raises(ValueError, match="outside MIDI range"):
        player.advance(250_000_000)
    assert [m.bytes() for m in player.stop(250_000_000)] == [[128, 120, 0]]


@pytest.mark.parametrize("port", ["transposition", "selection_offset"])
def test_fractional_offsets_are_rejected_before_time_or_music_changes(
    port: str,
) -> None:
    player = MidiPlayer(profile=parse_profile(Path("conformance/up.toml").read_text()))
    snapshot = player.model_dump_json()
    with pytest.raises(ValueError, match="whole"):
        player.control(
            50_000_000,
            arpeggiator_ports.ArpeggiatorControl.model_validate(
                {"port": port, "value": "1/2"}
            ),
        )
    assert player.model_dump_json() == snapshot


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


@pytest.mark.parametrize("occupied,admitted", [(4093, True), (4094, False)])
def test_capture_publication_events_reserve_space_for_the_whole_step(
    occupied: int,
    admitted: bool,
) -> None:
    ports = PerformancePorts(
        gate=Fraction(4, 5),
        density=Fraction(1),
        events=[
            arpeggiator_ports.ArpeggiatorOutput(at=0, port="step", index=0, revision=0)
        ]
        * occupied,
    )
    assert ports.begin_step(Fraction(1, 4), 1, 2, capture_ready=True) == admitted
    if admitted:
        ports.outcome(True)
    batch = ports.take_events()
    assert batch.exhausted != admitted
    assert len(batch.events) == (4096 if admitted else occupied)
    assert [e.port.value for e in batch.events[occupied:]] == (
        ["capture_ready", "step", "hit"] if admitted else []
    )
    assert ports.begin_step(Fraction(1, 2), 2, 2)
    ports.outcome(False)
    assert [e.port.value for e in ports.take_events().events] == ["step", "rest"]


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
