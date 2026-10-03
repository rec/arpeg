# arpeg

🧬 An expressive arpeggiator 🧬

The project design is in [plan/arpeggiator.md](plan/arpeggiator.md). The Python
package and the Rust event core render held and latched notes in ascending,
descending, and played order against an exact beat grid. The Rust core also
processes live held notes incrementally. Expressive capture and audio
realization are not implemented yet.

The Python package uses uFor's portable profile and capture contracts. The Rust
crate in `crates/arpeg-core` contains event decisions only; it has no dependency
on Python, MIDI device libraries, or audio processing.

The standalone `arpeg` executable validates supported profiles, renders
single-track metrical MIDI files containing note and tempo events, and plays
held-note arpeggios through CoreMIDI on macOS. Live mode reads MIDI channel 1,
outputs on channel 1, and uses an internal BPM clock. It polls every millisecond
and sends events immediately when due; input packet timestamps and future
CoreMIDI output timestamps are not used yet. Hardware timing and device behavior
have not been verified.

```sh
cargo run -p arpeg-midi -- validate conformance/up.toml
cargo run -p arpeg-midi -- render-file conformance/up.toml input.mid output.mid
cargo run -p arpeg-midi -- list-ports
cargo run -p arpeg-midi -- play conformance/up.toml SOURCE_INDEX DESTINATION_INDEX 120
```

Press Enter or Ctrl-C to stop live playback and release owned output notes.
Live mode currently accepts held-bank profiles; other bank modes remain
available in the file renderer.

## Development

```sh
uv sync
uv run pytest
uv run ruff check .
uv run ruff format --check .
uv run ty check src
cargo test --workspace
```
