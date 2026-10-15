import json
from fractions import Fraction
from pathlib import Path
from tomllib import loads

import mido
import pytest
from ufor.arpeggiator_capture import CapturedPhrase
from ufor.control import TempoMap

from arpeg.engine import EngineSettings, TokenArpeggiator
from arpeg.held import render_held
from arpeg.live import LiveArpeggiator
from arpeg.midi import MidiPlayer
from arpeg.profile import parse_profile


@pytest.mark.parametrize(
    "case",
    loads(Path("conformance/swing.toml").read_text())["cases"],
    ids=lambda c: c["name"],
)
@pytest.mark.parametrize("polling", [False, True])
def test_swing_traces_match_tokens_midi_and_snapshots(
    case: dict[str, object], polling: bool
) -> None:
    profile = parse_profile(Path("conformance/up.toml").read_text())
    profile = type(profile).model_validate(
        profile.model_dump()
        | {
            "body": profile.body.model_dump()
            | {"rhythm": case["rhythm"], "gate": case["gate"]}
        }
    )
    for engine in [
        LiveArpeggiator(profile=profile),
        TokenArpeggiator(
            settings=EngineSettings(rhythm=profile.body.rhythm, gate=profile.body.gate)
        ),
    ]:
        if isinstance(engine, LiveArpeggiator):
            engine.note_on(Fraction(0), 60, 100)
        else:
            engine.press(Fraction(0), 1000, 9000, "λ")
        events = []
        cycles = []
        through = Fraction(case["through"])
        while polling and engine.now + Fraction(1, 17) < through:
            events.extend(engine.advance(engine.now + Fraction(1, 17)))
            assert engine.ports is not None
            cycles.extend(
                str(e.at)
                for e in engine.ports.take_events().events
                if e.port == "cycle"
            )
            engine = type(engine).model_validate_json(engine.model_dump_json())
        events.extend(engine.advance(through))
        assert engine.ports is not None
        cycles.extend(
            str(e.at) for e in engine.ports.take_events().events if e.port == "cycle"
        )
        assert [[str(e.at), e.kind] for e in events] == case["events"]
        assert all(e.source_id == 0 for e in events)
        assert cycles == case["cycles"]


@pytest.mark.parametrize(
    ("at", "start"),
    [("1/4", "1/3"), ("1/3", "1/3"), ("2/5", "1/2"), ("1/2", "1/2"), ("3/5", "5/6")],
)
def test_seek_and_pause_land_on_the_next_swung_start(at: str, start: str) -> None:
    profile = parse_profile(Path("conformance/up-swing.toml").read_text())
    for command in ["relocate", "pause"]:
        engine = LiveArpeggiator(profile=profile)
        engine.note_on(Fraction(0), 60, 100)
        getattr(engine, command)(Fraction(at))
        restored = LiveArpeggiator.model_validate_json(engine.model_dump_json())
        assert restored.next_deadline() == Fraction(start)
        assert [(e.kind, e.at) for e in restored.advance(Fraction(start))] == [
            ("on", Fraction(start))
        ]


def test_file_rendering_excludes_a_delayed_start_beyond_the_horizon() -> None:
    case = json.loads(Path("conformance/held-chord.json").read_text())
    notes = render_held(
        parse_profile(Path("conformance/up-swing.toml").read_text()),
        CapturedPhrase.model_validate(case["phrase"]),
        TempoMap.model_validate(case["tempo"]),
        Fraction(3, 10),
        "synth",
    )
    assert [(n.onset, n.gate_end) for n in notes] == [(Fraction(0), Fraction(4, 15))]


def test_captured_gestures_fit_the_interval_after_the_swung_start() -> None:
    profile = parse_profile(Path("conformance/history-wind.toml").read_text())
    profile = profile.model_copy(
        update={
            "body": profile.body.model_copy(
                update={
                    "rhythm": parse_profile(
                        Path("conformance/up-swing.toml").read_text()
                    ).body.rhythm
                }
            )
        }
    )
    player = MidiPlayer(profile=profile)
    player.accept(0, mido.Message.from_bytes([144, 60, 100]))
    player.accept(50_000_000, mido.Message.from_bytes([128, 60, 20]))
    player.advance(500_000_000)
    notes = player.take_events().notes
    assert [(n.port.value, n.at) for n in notes] == [
        ("note_start", Fraction(1, 3)),
        ("note_end", Fraction(7, 15)),
        ("note_start", Fraction(1, 2)),
        ("note_end", Fraction(23, 30)),
        ("note_start", Fraction(5, 6)),
        ("note_end", Fraction(29, 30)),
        ("note_start", Fraction(1)),
    ]
