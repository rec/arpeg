# arpeg

🧬 An expressive arpeggiator 🧬

The project design is in [plan/arpeggiator.md](plan/arpeggiator.md). The Python
package and the Rust event core currently render held, ascending note patterns
against an exact beat grid. MIDI ports, expressive capture, and audio realization
are not implemented yet.

The Python package uses uFor's portable profile and capture contracts. The Rust
crate in `crates/arpeg-core` contains event decisions only; it has no dependency
on Python, MIDI device libraries, or audio processing.

## Development

```sh
uv sync
uv run pytest
uv run ruff check .
uv run ruff format --check .
uv run ty check src
cargo test --workspace
```
