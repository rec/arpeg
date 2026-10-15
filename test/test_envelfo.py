from fractions import Fraction
from pathlib import Path
from tomllib import loads

import mido
import pytest
from envelfo import patch
from ufor import arpeggiator_ports, control, envelfo
from ufor.segments import Segment

from arpeg.envelfo import EnveLFOBridge
from arpeg.midi import MidiPlayer
from arpeg.profile import parse_profile


@pytest.mark.parametrize("poll_us", [None, 1000])
def test_composition_trace_and_restoration(poll_us: int | None) -> None:
    trace = loads(Path("conformance/envelfo-composition.toml").read_text())
    player = MidiPlayer(profile=parse_profile(trace["profile"]))
    bridge = _bridge()
    previous = 0
    for action in trace["actions"]:
        tick = action["at_us"] * 1000
        output = []
        if poll_us is not None:
            for t in range(previous + poll_us * 1000, tick, poll_us * 1000):
                output.extend(player.advance(t))
                batch = player.take_events()
                assert not batch.exhausted
                assert not any(e.port == "hit" for e in batch.events)
        for data in action.get("data", []):
            output.extend(player.accept(tick, mido.Message.from_bytes(data)))
        if action.get("stop"):
            output.extend(player.stop(tick))
        player.schedule(tick, tempo=action.get("tempo"))
        player = MidiPlayer.model_validate_json(player.model_dump_json())
        batch = player.take_events()
        assert not batch.exhausted
        at = Fraction(action["at_us"], 1_000_000)
        for event in batch.events:
            bridge.receive(at, event)
        result = bridge.observe(at, next_delivery=at + Fraction(1, 1000))
        assert result.values == pytest.approx(
            {"breath": action["breath"], "accent": action["accent"]}
        )
        output.extend(player.realize(result.controls))
        assert [m.bytes() for m in output] == action["output"], action
        player = MidiPlayer.model_validate_json(player.model_dump_json())
        bridge = EnveLFOBridge.model_validate_json(bridge.model_dump_json())
        previous = tick


def test_host_tempo_maps_each_child_without_owning_a_second_clock() -> None:
    bridge = _bridge()
    attack = bridge.definition.envelfos["attack"]
    assert isinstance(attack, envelfo.Envelope)
    definition = bridge.definition.model_copy(
        update={
            "envelfos": {
                **bridge.definition.envelfos,
                "attack": attack.model_copy(
                    update={
                        "clock": control.Clock.beats,
                        "segments": [Segment(duration="1/4 beat", to=1)],
                    }
                ),
            }
        }
    )
    tempo = control.TempoMap.model_validate(
        {"points": [{"at_seconds": 0, "beat": 0, "bpm": 120}]}
    )
    bridge.definition = definition
    bridge.state = patch.initial_patch(definition, Fraction(0), tempo_map=tempo)
    bridge.receive(
        Fraction(0),
        arpeggiator_ports.ArpeggiatorOutput(at=0, port="hit", index=0, revision=1),
        tempo_map=tempo,
    )
    result = bridge.observe(
        Fraction(1, 16), next_delivery=Fraction(1, 8), tempo_map=tempo
    )
    assert result.values == {"breath": 0.125, "accent": 0.5}


def test_pending_delivery_survives_snapshot_without_replaying_a_hit() -> None:
    bridge = _bridge()
    bridge.definition = bridge.definition.model_copy(
        update={
            "envelfos": {
                **bridge.definition.envelfos,
                "wave": envelfo.LFO(
                    rate=1,
                    shape="triangle",
                    center=0.5,
                    depth=0.5,
                    markers=[envelfo.Marker(name="tick", position=Fraction(1, 4))],
                ),
                "held": envelfo.Latch(input="wave"),
            },
            "outputs": {**bridge.definition.outputs, "captured": "held"},
            "events": [
                envelfo.PatchEventConnection(
                    source="wave.tick", target="held", action="capture", delay="50 ms"
                )
            ],
        }
    )
    bridge.state = patch.initial_patch(bridge.definition, Fraction(0))
    hit = arpeggiator_ports.ArpeggiatorOutput(at=0, port="hit", index=0, revision=1)
    bridge.receive(Fraction(0), hit)
    bridge.observe(Fraction(1, 4), next_delivery=Fraction(3, 10))
    assert bridge.state.pending
    saved = bridge.model_dump_json()
    restored = EnveLFOBridge.model_validate_json(saved)
    expected = bridge.observe(Fraction(3, 10), next_delivery=Fraction(2, 5))
    actual = restored.observe(Fraction(3, 10), next_delivery=Fraction(2, 5))
    assert actual == expected
    assert actual.values["captured"] == pytest.approx(0.6)
    assert not restored.state.pending
    assert EnveLFOBridge.model_validate_json(saved).state.pending


def test_same_time_hits_and_invalid_samples_preserve_state() -> None:
    bridge = _bridge()
    for index in range(2):
        bridge.receive(
            Fraction(0),
            arpeggiator_ports.ArpeggiatorOutput(
                at=0, port="hit", index=index, revision=1
            ),
        )
    assert bridge.state.children["attack"].runtime.ordinal == 1
    bridge.definition = bridge.definition.model_copy(
        update={"envelfos": {**bridge.definition.envelfos, "wave": envelfo.LFO(rate=1)}}
    )
    before = bridge.model_dump_json()
    with pytest.raises(ValueError, match="breath"):
        bridge.observe(Fraction(3, 4), next_delivery=Fraction(1))
    assert bridge.model_dump_json() == before


def _bridge() -> EnveLFOBridge:
    definition = patch.PreparedPatch(
        envelfos={
            "wave": envelfo.LFO(rate=1, shape="triangle", center=0.5, depth=0.5),
            "attack": envelfo.Envelope(
                start="event",
                retrigger="reset",
                segments=[Segment(duration="250 ms", to=1)],
            ),
        },
        outputs={"breath": "wave", "accent": "attack"},
    )
    return EnveLFOBridge(
        definition=definition,
        state=patch.initial_patch(definition, Fraction(0)),
        hit_target="attack",
        expression={"breath": "breath", "pressure": "accent"},
    )
