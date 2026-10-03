# arpeg

🧬 An expressive arpeggiator 🧬

The project design is in [plan/arpeggiator.md](plan/arpeggiator.md). The Python
package and the Rust event core currently render held and latched notes in
ascending, descending, and played order against an exact beat grid. MIDI ports,
expressive capture, and audio realization are not implemented yet.

The Python package uses uFor's portable profile and capture contracts. The Rust
crate in `crates/arpeg-core` contains event decisions only; it has no dependency
on Python, MIDI device libraries, or audio processing.

The standalone `arpeg` executable currently validates supported profiles and
renders single-track metrical MIDI files containing note and tempo events. It
rejects other input events until their capture and ownership rules are in place.

```sh
cargo run -p arpeg-midi -- validate conformance/up.toml
cargo run -p arpeg-midi -- render-file conformance/up.toml input.mid output.mid
```

## Development

```sh
uv sync
uv run pytest
uv run ruff check .
uv run ruff format --check .
uv run ty check src
cargo test --workspace
```
