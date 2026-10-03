# arpeg

🧬 An expressive arpeggiator 🧬

The project design is in [plan/arpeggiator.md](plan/arpeggiator.md). The Python
package and the Rust event core render held and latched notes in ascending,
descending, and played order against an exact beat grid. Both cores process
live held and latched notes incrementally. Expressive capture and audio
realization are not implemented yet.

The Python package uses uFor's portable profile and capture contracts. The Rust
crate in `crates/arpeg-core` contains event decisions only; it has no dependency
on Python, MIDI device libraries, or audio processing.

The readable Python live engine is [src/arpeg/live.py](src/arpeg/live.py).
`LiveArpeggiator` accepts a uFor profile and provides `note_on`, `note_off`,
`advance`, `clear`, and `stop` methods with exact beat times. Its behavior is
covered in [test/test_live.py](test/test_live.py) and the shared
[live traces](conformance/live-classic.json). The CoreMIDI executable uses the Rust
engine, so Python is not required when playing from MIDI ports.

The standalone `arpeg` executable validates supported profiles, renders
single-track metrical MIDI files containing note and tempo events, and plays
held or latched note arpeggios through CoreMIDI on macOS. Live mode reads MIDI channel 1,
outputs on channel 1, and uses an internal BPM clock. It polls every millisecond
and sends events immediately when due; input packet timestamps and future
CoreMIDI output timestamps are not used yet. Hardware timing and device behavior
have not been verified.

```sh
cargo run -p arpeg-midi -- validate conformance/up.toml
cargo run -p arpeg-midi -- render-file conformance/up.toml input.mid output.mid
cargo run -p arpeg-midi -- list-ports
cargo run -p arpeg-midi -- play conformance/up.toml SOURCE_INDEX DESTINATION_INDEX 120
cargo run -p arpeg-midi -- play conformance/live-latch.toml SOURCE_INDEX DESTINATION_INDEX 120
```

Enter `clear` to empty a latched bank and release its owned output notes. Press
Enter or Ctrl-C to stop live playback. The default `retrigger = "on_empty"`
continues selection through chord edits; `retrigger = "bank_edit"` restarts
selection at the first note on the next grid step without moving the grid.
File rendering rejects `bank_edit` until it can reproduce the same live
decisions from a complete input trace.

## Development

```sh
uv sync
uv run pytest
uv run ruff check .
uv run ruff format --check .
uv run ty check src
cargo test --workspace
```
