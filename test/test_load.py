from pathlib import Path

from meedee import codecs
from meedee.messages import (
    ControlChange,
    NoteOff,
    NoteOn,
)

from arpeg.midi import MidiPlayer
from arpeg.profile import parse_profile


def test_dense_recorded_breath_keeps_equal_samples_and_cancels_them_on_stop() -> None:
    player = MidiPlayer(
        profile=parse_profile(Path("conformance/phrase-wind.toml").read_text())
    )
    player.capture(0, "record")
    player.accept(0, NoteOn(channel=1, note=60, velocity=100))
    for tick in range(1, 2001):
        player.accept(
            tick * 1_000_000, ControlChange(channel=1, controller=2, value=64)
        )
    player.accept(2_001_000_000, NoteOff(channel=1, note=60, velocity=20))
    player.capture(2_002_000_000, "commit")
    assert [list(codecs.encode(m)) for m in player.advance(2_125_000_000)] == [
        [144, 60, 100]
    ]
    breath = player.advance(2_175_000_000)
    assert len(breath) == 1000
    assert all(list(codecs.encode(m)) == [176, 2, 64] for m in breath)
    assert [list(codecs.encode(m)) for m in player.stop(2_175_000_000)] == [
        [128, 60, 0]
    ]
    assert player.advance(3_000_000_000) == []
    notes = player.take_events().notes
    assert [n.port for n in notes] == ["note_start", "note_end"]
    assert notes[0].occurrence == notes[1].occurrence
