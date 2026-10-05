# arpeg

🧬 An expressive arpeggiator 🧬

The project design is in [plan/arpeggiator.md](plan/arpeggiator.md). The Python
package and the Rust event core render held and latched notes in ascending,
descending, and played order against an exact beat grid. Both cores process
live held and latched notes incrementally. Python also captures expressive MIDI
phrases with controller entry state and retained source events. Marked sample
regions can now be lowered to source-frame notes and realized through enge's
existing sampler. Live sample playback is not wired yet.

The Python package uses uFor's portable profile and capture contracts. The Rust
crate in `crates/arpeg-core` contains event decisions only; it has no dependency
on Python, MIDI device libraries, or audio processing.

The readable Python live engine is [src/arpeg/live.py](src/arpeg/live.py).
`LiveArpeggiator` accepts a uFor profile and provides `note_on`, `note_off`,
`advance`, `clear`, and `stop` methods with exact beat times. Its behavior is
covered in [test/test_live.py](test/test_live.py) and the shared
[live traces](conformance/live-classic.json). The CoreMIDI executable uses the Rust
engine, so Python is not required when playing from MIDI ports.

Held and latched banks also accept [Euclidean rhythm](conformance/euclidean.toml)
in Python and Rust, for live playing or file rendering. Three pulses in eight
steps gives `10010010`; positive rotation moves hits later. Rests leave note
selection unchanged, while gates still release at their scheduled times. The
mask repeats on the local step clock, including through empty banks and bank
edits. Zero pulses is silence and full pulses is the ordinary grid. Exact masks,
rotations, and event traces are shared in [the fixture](conformance/euclidean.json).
Recorded-history playback currently requires grid rhythm.

[Custom step patterns](conformance/custom-steps.toml) repeat a list of `hit`,
`rest`, and `tie` steps, each with an exact beat `duration`. A hit can specify
`repeats` to divide its duration into evenly spaced attacks of one selected
note. Selection advances once per hit; rests and ties do not select a new note.
Gates use each attack's subdivision duration. Consecutive ties sustain the
final attack through the chain, with the gate fraction applied to the final
tie; an already longer gate is preserved. Ties can cross the cycle boundary,
and leading ties without an occurrence are silent. Stop, clear, or an empty
bank cancels pending attacks and releases owned notes. Chord edits preserve an
already selected repeat group. The [shared traces](conformance/custom-steps.json)
check the Python and Rust schedulers with irregular polling.
Custom patterns currently support live held and latched banks; file rendering
and recorded history reject them.

[Weighted walks](conformance/weighted-walk.toml) move through the bank in pitch
order, breaking equal-pitch ties by source identity. `moves` contains signed
rank offsets, `weights` contains their positive integer weights, and movement
wraps around the bank. `start = "lowest"` plays the lowest note first;
`start = "move"` applies a weighted move from that position before playing.
When a chord edit removes the selected note, `on_remove = "lowest"` restarts
at the lowest note; `on_remove = "rank"` keeps the previous rank modulo the
new bank size, then applies a move. Both defaults are `"lowest"`. A retained
note continues from its current rank. Bank-edit retrigger uses the start policy.

`probability = "2/3"` admits each eligible hit with that exact probability.
The default is `1`; `0` is silence. Rejected hits leave selection unchanged
and do not move the rhythm clock. Repeats share one decision and one note.
Rests, ties, and empty-bank steps do not consume chance decisions. An explicit
integer `seed` is required for probability strictly between zero and one and
for walks with multiple moves. Probability and walk choices have separate
counters; profile name and bank revision also enter each choice. The same
profile and input trace therefore reproduce the same decisions regardless of
polling intervals. Python snapshots retain these counters and traversal state.
The [random contract](conformance/random.md) and
[shared traces](conformance/chance-walk.json) specify Python/Rust parity.
Probability and walks currently support live held and latched banks;
file rendering and recorded history reject them.

[src/arpeg/capture.py](src/arpeg/capture.py) records a MIDI phrase through
`MidiCapture.accept` and closes it with `finish`. Its profile declares whether
overlapping onsets hand off a monophonic segment or remain independent notes.
The original MIDI bytes stay in the captured ledger, including velocity-zero
note-on releases. The Python capture fixture checks use the wind and gap traces
in `conformance/`.

The Python [gesture renderer](src/arpeg/gesture.py) reorders completed MIDI
notes in source timing or fits their gestures to an output gate. It restores
known controller entry values before each onset, retains note-local event times,
and allocates a separate channel when gestures overlap. Unowned gap events remain
in the captured phrase and are not sent into an unrelated output note. It can
instead use live breath and bend, or replay the original MIDI ledger unchanged.
For a single-channel destination, `overlap="handoff"` ends the old output note
before initializing the new gesture on channel 1.

[src/arpeg/bank.py](src/arpeg/bank.py) records history or phrase takes. Completed
history notes wait for the declared capture tail, and replace, overdub, undo,
and clear changes become visible at `publish_step`. `select_step` chooses a
source note from the published revision; callers can place it through the
gesture renderer at an output onset.

[src/arpeg/history.py](src/arpeg/history.py) is the Python reference for live
recorded history. `LiveHistoryArpeggiator` captures channel 1 MIDI, selects
completed notes at exact grid steps, and fits their recorded breath and bend
gestures to each output gate. It owns one sounding output note at a time:
each new step ends the old note before starting the next gesture. `clear`
forgets the history and releases the output note while continuing to capture
new input. The Rust event core implements the same step and handoff rules.

[src/arpeg/marked_sample.py](src/arpeg/marked_sample.py) turns ordered frame
markers into identified `SourceNote` regions. An explicit marker at frame zero
accounts for any prefix, and the last region ends at the asset boundary.
Selection keys order regions without claiming an acoustic pitch. The Rust core
checks the same [region fixture](conformance/marked-sample.json). The enge
`sample_regions` adapter consumes these notes through its prepared sampler;
adjacent source regions share a continuous cursor run. Source-rate identity and
reordered playback have one-second WAV regressions on both sampler backends.

The standalone `arpeg` executable validates supported profiles, renders
single-track metrical MIDI files containing note and tempo events, and plays
classic or recorded-history arpeggios through CoreMIDI on macOS. Live mode reads MIDI
channel 1, outputs on channel 1, and uses an internal BPM clock. It polls every millisecond
and sends events immediately when due; input packet timestamps and future
CoreMIDI output timestamps are not used yet. Hardware timing and device behavior
have not been verified.

```sh
cargo run -p arpeg-midi -- validate conformance/up.toml
cargo run -p arpeg-midi -- render-file conformance/up.toml input.mid output.mid
cargo run -p arpeg-midi -- list-ports
cargo run -p arpeg-midi -- play conformance/up.toml SOURCE_INDEX DESTINATION_INDEX 120
cargo run -p arpeg-midi -- play conformance/live-latch.toml SOURCE_INDEX DESTINATION_INDEX 120
cargo run -p arpeg-midi -- play conformance/euclidean.toml SOURCE_INDEX DESTINATION_INDEX 120
cargo run -p arpeg-midi -- play conformance/custom-steps.toml SOURCE_INDEX DESTINATION_INDEX 120
cargo run -p arpeg-midi -- play conformance/weighted-walk.toml SOURCE_INDEX DESTINATION_INDEX 120
cargo run -p arpeg-midi -- play conformance/history-wind.toml SOURCE_INDEX DESTINATION_INDEX 120
```

Enter `clear` to empty a latched or history bank and release its owned output notes. Press
Enter or Ctrl-C to stop live playback. The default `retrigger = "on_empty"`
continues selection through chord edits; `retrigger = "bank_edit"` restarts
selection at the first note on the next grid step without moving the grid.
File rendering rejects `bank_edit` until it can reproduce the same live
decisions from a complete input trace. Recorded history is live only; file
rendering rejects its profile. The current history preset captures CC2 breath
and pitch bend from channel 1 and fits their recorded note gestures to each
step. Channel 1 handoff ends overlapping gestures rather than assigning them
independent MIDI channels.

## Development

```sh
uv sync
uv run pytest
uv run ruff check .
uv run ruff format --check .
uv run ty check src
cargo test --workspace
```
