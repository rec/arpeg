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
[live traces](conformance/live-classic.json). The standalone executable uses the Rust
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

[Alternating selection](conformance/alternating.toml) walks up and down through
the chord in pitch order. With `repeat_endpoints = false` (the default), three
notes play `60, 64, 67, 64, 60, 64…`. Setting it to `true` plays
`60, 64, 67, 67, 64, 60, 60…`. Equal pitches remain distinct source notes.
The first admitted hit starts at the lowest note, moving upward. Ordinary
chord edits preserve direction and continue to the nearest note beyond the
previous pitch/identity; if none remains, direction reverses. Only a retained
endpoint can be repeated. A single-note bank keeps playing that note, and an
empty bank resets traversal. `retrigger = "bank_edit"` restarts at the lowest
note. Repeats reuse a selection, and rests, ties, and chance-rejected hits do
not advance traversal. Python snapshots retain direction. The
[shared traces](conformance/alternating.json) check both engines under irregular
polling. Alternating selection currently supports live held and latched banks.

[Inside-out](conformance/inside-out.toml) selection visits notes from the center
outward; [outside-in](conformance/outside-in.toml) visits the edges first.
Use `selection = { kind = "inside_out" }` or `kind = "outside_in"`.
For pitches `60, 64, 67, 72`, their repeating orders are respectively
`64, 67, 60, 72` and `60, 72, 64, 67`. For an odd-sized chord, inside-out
starts at the center, then visits the lower side before the upper side at
each distance; outside-in always starts on the lower side. Equal pitches
are ordered by source identity. Each cycle visits every note once.
Chord edits rebuild the order and continue after the selected identity if it
survives; removal restarts at the beginning. Bank-edit retrigger, clear, and an
empty bank also restart. Single-note banks repeat that note. Rests, ties,
chance-rejected hits, and repeated attacks follow the existing selection rules.
Both Python and Rust support these orders for live held and latched banks.
File rendering and recorded history reject them. The
[shared traces](conformance/center-edge.json) specify their exact behavior.

[Index patterns](conformance/index-pattern.toml) cycle through explicit chord
positions: `selection = { kind = "index_pattern", indices = [0, 2, 1, 2] }`.
Positions start at zero, ordered by ascending pitch and then source identity.
`boundary = "wrap"` is the default: each index is taken modulo the bank size.
Set `boundary = "rest"` to skip a missing position and advance to the next slot.
Ordinary chord edits preserve the pattern position; bank-edit retrigger, an empty
bank, clear, and stop reset it. Repeats reuse one selection. Rhythm rests, ties,
masked steps, and chance-rejected hits leave the position unchanged. Python
snapshots retain it, including after a missing-index rest. Both engines support
live held and latched banks; file rendering, history, and marked samples reject
this selection. [Shared traces](conformance/index-pattern.json) check the order
and both boundary policies under irregular polling.

[Shuffle selection](conformance/shuffle.toml) visits every note once per cycle.
`selection = { kind = "shuffle" }` shuffles again each cycle; `mode = "once"`
repeats the initial order. An explicit integer `seed` is required. The default
allows the same source note at the end of one cycle and the beginning of the
next. Set `no_repeat = true` to prevent consecutive selections of that source
when more than one note is available. A hit's repeats still reuse its selection.

`on_edit = "restart"` is the default: chord edits start a fresh shuffled cycle.
With `on_edit = "preserve"`, surviving notes retain their order, removed notes
disappear, and added notes are inserted randomly into the unplayed portion.
The updated complete order is reused in `once` mode. Bank-edit retrigger, an
empty bank, clear, and stop discard the order. Rhythm rests, ties, masked steps,
and chance-rejected hits leave the queue unchanged. Random counters persist
through resets and in Python snapshots. Both engines support live held and
latched banks; file rendering, history, and marked samples reject shuffle.
[Shared traces](conformance/shuffle.json) check both modes and edit policies.

[Weighted choice](conformance/choice.toml) independently selects a note on each
admitted hit: `selection = { kind = "choice", weights = [4, 1] }`.
Weights follow ascending pitch/source identity ranks and are positive integers
through `4294967295`. The default weights `[1]` make every note equally likely.
`extend = "ones"` supplies weight 1 for unspecified ranks; `extend = "repeat"`
cycles the weight list across the bank. Extra weights beyond the bank are unused.
Thus `[4, 1]` gives three notes weights `[4, 1, 1]` by default or `[4, 1, 4]`
with repetition. Chord edits update rank weights on the next admitted hit.

An explicit integer `seed` is required. Repeated selections are allowed by
default. `no_repeat = true` excludes the previous source identity before drawing
when another note is available; it preserves the remaining notes' original rank
weights. A single-note bank still plays. A hit's repeats reuse its choice, and
rhythm rests, ties, masked steps, and chance-rejected hits consume no choice.
Bank-edit retrigger, an empty bank, clear, and stop forget the previous identity
while retaining the random counter. Python snapshots retain both. Both engines
support live held and latched banks; file rendering, history, and marked samples
reject choice. [Shared traces](conformance/choice.json) specify the exact draws.

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
history notes wait for the declared capture tail, and replace, overdub, and undo
changes become visible at `publish_step`. Clear empties the published bank
immediately. `select_step` chooses a
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
classic or recorded-history arpeggios through midir on Linux, Windows, and macOS.
The Python `arpeg-python` host uses mido and its RtMidi backend to run the
reference engines with the same presets on those platforms. Both hosts read MIDI
channel 1 and output on channel 1. The default clock uses internal BPM; both
hosts also accept external MIDI Clock and transport. They timestamp
messages when the input callback runs, poll every millisecond, and send events
immediately when due. Driver timestamps and future output timestamps are not
used yet. Hardware timing and device behavior have not been verified.

Python MIDI port access requires the `midi` extra. The default package installs
the reference engines, mido, and the CLI without the native RtMidi backend:

```sh
uv sync --extra midi
uv run --extra midi arpeg-python list-ports
uv run --extra midi arpeg-python play --profile conformance/up.toml --source 0 --destination 1 --bpm 120
uv run --extra midi arpeg-python play --profile conformance/history-wind.toml --source 0 --destination 1 --bpm 120
```

Use the indices listed by the host you intend to run; Python and Rust port
enumeration need not use the same order. Mido's RtMidi backend filters incoming
active-sensing messages; received note, controller, bend, clock, and SysEx
messages are handled through its portable API. Native driver selection is
inside the MIDI libraries, with no OS-specific calls in arpeg.

```sh
cargo run -p arpeg-midi -- validate conformance/up.toml
cargo run -p arpeg-midi -- render-file conformance/up.toml input.mid output.mid
cargo run -p arpeg-midi -- list-ports
cargo run -p arpeg-midi -- play conformance/up.toml SOURCE_INDEX DESTINATION_INDEX 120
cargo run -p arpeg-midi -- play conformance/live-latch.toml SOURCE_INDEX DESTINATION_INDEX 120
cargo run -p arpeg-midi -- play conformance/euclidean.toml SOURCE_INDEX DESTINATION_INDEX 120
cargo run -p arpeg-midi -- play conformance/custom-steps.toml SOURCE_INDEX DESTINATION_INDEX 120
cargo run -p arpeg-midi -- play conformance/weighted-walk.toml SOURCE_INDEX DESTINATION_INDEX 120
cargo run -p arpeg-midi -- play conformance/alternating.toml SOURCE_INDEX DESTINATION_INDEX 120
cargo run -p arpeg-midi -- play conformance/history-wind.toml SOURCE_INDEX DESTINATION_INDEX 120
```

### Live clock and transport

For external synchronization, add `--clock external` to either play command.
Clock and transport come from the note input by default. Add `--clock-source INDEX`
to use a separate input; that port accepts only Clock, Start, Continue, Stop and
Song Position Pointer, while the note port supplies performance events. Selecting
the note port's own index opens it once.

MIDI Clock uses [24 pulses per quarter note](https://midi.org/about-midi-part-3midi-messages).
External playback initially waits for Start or Continue, then the first pulse
anchors the current beat. The second pulse establishes an interval. Between
pulses, the clock interpolates using the latest interval, capped at the next
expected pulse. Late pulses never move playback backwards. This conservative
estimator can stall with jitter; it does not extrapolate through missing pulses.
After `--clock-timeout-ms` (default 500), it stops and releases owned notes.
Fresh pulses alone do not restart it: send Start or Continue to reacquire.

Enter `start` to restart rhythm and traversal while retaining the source bank;
`pause` releases output and freezes beats and traversal; `continue` resumes them.
The corresponding MIDI transport messages do the same. Song Position Pointer
uses sixteenth-note units, releases output, resets traversal, and resumes at the
next eligible rhythm boundary. Seeks discard pending repeats and gestures rather
than emitting crossed attacks. Random counters remain intact across transport
changes. External Continue and seeks wait for a pulse before resuming.

With the internal clock, enter `tempo 90` to change BPM without changing beat
phase. Recorded source timestamps remain microseconds; fitted gestures and gates
follow playback beats, including through tempo changes and pauses. Capture remains
available while playback is stopped. Python player snapshots retain both engine
and clock state, including clock observations; observation storage is capped at
one million messages. [Shared transport traces](conformance/transport.toml) check
the same beats and MIDI bytes in both implementations.

Enter `clear` to empty a latched or history bank and release its owned output notes.
Press Enter or Ctrl-C to exit live playback. The default `retrigger = "on_empty"`
continues selection through chord edits; `retrigger = "bank_edit"` restarts
selection at the first note on the next grid step without moving the grid.
File rendering rejects `bank_edit` until it can reproduce the same live
decisions from a complete input trace. Recorded history is live only; file
rendering rejects its profile. The current history preset captures CC2 breath
and pitch bend from channel 1 and fits their recorded note gestures to each
step. Channel 1 handoff ends overlapping gestures rather than assigning them
independent MIDI channels.

### Live expressive performance

Both hosts now follow channel 1 breath (CC2), pitch bend and channel pressure
with held or latched notes. Known values are restored before each generated
onset, in breath/bend/pressure order; unseen values are left alone. Changes
follow the owned sounding note. They are remembered during rests and pauses
without being sent to an unrelated voice. Other input channels, sustain pedal,
polyphonic pressure and other controllers are not live expression lanes yet.

[live-wind](conformance/live-wind.toml) latches overlapping keys into a chord and
uses live expression. Its gate exceeds the step, demonstrating channel 1
handoff. Scheduled releases belonging to an earlier output occurrence cannot
cut off the new one, even when the pitch repeats.

[history-live-wind](conformance/history-live-wind.toml) captures recent completed
notes and plays their pitches and velocities with your current breath, bend and
pressure. The recorded controller ledger remains stored but its gestures are
not played in this mode. [history-wind](conformance/history-wind.toml) instead
restores recorded entry values and fits recorded changes, now including channel
pressure, to each gate. Controllers arriving outside a completed capture cell
remain in the raw ledger and can establish the next note's entry state; they do
not extend that cell or become part of its gesture.

Use either preset as the `--profile` value in Python or the profile argument in
Rust. [Shared performance traces](conformance/performance.toml) cover live and
recorded expression, handoff, unknown state, pause and clear cleanup. The live
hosts still have one independent expression owner on channel 1. MPE and MIDI 2.0
remain later work.

### Live phrase capture

Use [phrase-wind](conformance/phrase-wind.toml) as the live profile in either
Python or Rust. The phrase bank starts empty and listens to channel 1:

- `record` starts a new take immediately. The previous committed bank keeps playing.
- `commit` ends the take and replaces the bank at the next grid step.
- `overdub` ends the take and adds its notes to the bank at the next grid step.
- `undo` removes the latest committed take at the next grid step, restoring the
  preceding bank. An ongoing recording continues.
- `clear` discards the bank and any recording and releases owned output immediately.

Committing closes any still-held captured notes. An empty replacement makes
subsequent steps silent; an already sounding gesture keeps its scheduled release.
Capture controls do not cut off sounding gestures. New notes hand off channel 1
at their scheduled step as usual. The preset restarts traversal on each published
bank edit; `retrigger = "on_empty"` preserves traversal through edits.

Known breath, bend, and channel pressure are sampled at the recording boundary
and stored as entry state. Changes within the take retain their MIDI bytes and
source timing; playback fits their gestures to the gate. Set expression
`source = "current"` to use live expression instead. Recording works while
transport is paused, and commits wait for a step after playback resumes.
Input is captured material rather than MIDI through.

The same controls are available through `MidiPlayer.capture(time, command)` in
Python, Rust, and WebAssembly. Python uses nanoseconds; Rust and WebAssembly use
microseconds. The native hosts report recording status, published note count,
and bank revision after commands. Recording is bounded to one million events
between clears and 128 committed takes, including undo history. Undo frees a take
slot; clear frees both budgets. Capture identities stay unique after undo and clear.
The raw captured source remains in the bank, independent of rendered output.
[Shared phrase traces](conformance/phrase.toml) cover replacement, overdub, undo,
entry state, pause, boundary timing, and cleanup with coarse and frequent polling.
Phrase playback currently uses a grid with classic note orders, fit timing,
and carried gaps, matching history playback.

### Motion control ports

Live held, latched, history, and phrase engines accept two scalar input ports:

- `gate`: a nonnegative exact gate fraction. Values above 1 allow overlap.
- `density`: hit probability between 0 and 1. It starts at the preset's
  probability and replaces it when changed. Fractional density requires an
  explicit preset seed and uses the existing probability draw counter.

Changes publish at the next unprocessed step, including a control delivered
before that step at the same time. The latest queued value per port wins.
Sounding notes, fitted gestures, tied gates, and pending repeat groups keep their
realized parameters. A pause retains queued controls; Start or song-position
relocation cancels them. Density rejections leave note selection unchanged.

Use [motion-ports](conformance/motion-ports.toml) in either native MIDI host and
enter `gate 1/2` or `density 2/3`. The preset includes the required seed.
For a Python host:

```python
from ufor.arpeggiator_ports import ArpeggiatorControl

control = ArpeggiatorControl.model_validate({"port": "density", "value": "2/3"})
messages = player.control(now_ns, control)
messages.extend(player.advance(now_ns))
batch = player.take_events()
for event in batch.events:
    print(event.at, event.port, event.index, event.revision)
```

Rust accepts `player.control(now_us, InputPort::Density, value)` and returns the
same MIDI output plus a separate `take_events()` batch. Both pure event engines
also expose controls and their `ports` state. Python snapshots preserve effective
values, queued controls, undrained events, and probability counters.

Every opportunity produces `step`, then `hit` when a source note was admitted or
`rest` otherwise. Ties produce only `step`; repeats share one `hit`. Events carry
exact beat time, rhythm index, and bank revision. They describe source admission;
a MIDI destination may subsequently hand off an older output note.

The host samples its Motion before the target step and routes collected events
to other Motions. Convert sampled scalar values to explicit rational values;
there is no hidden float rounding or embedded Motion graph evaluator. Feedback
needs an explicit delay of at least one scheduling quantum and a bounded host
event budget.

Drain after each operation. The output buffer holds 4096 events and reserves two
slots before admitting a step. On exhaustion, new step admissions stop while due
releases and already realized repeats continue. `batch.exhausted` reports the
condition; draining permits future steps without replaying skipped attacks.
Native MIDI hosts drain automatically and print a diagnostic on exhaustion.
[Shared traces](conformance/ports.toml) cover gate edits, density and seeded
decisions, ties, repeats, captured material, pause, seeks, and polling parity.

### Preset header defaults

Presets may omit `kind`; it defaults to `"arpeggiator"`. They may also omit
`name` when loaded from a file: `presets/bright.v2.toml` becomes `"bright.v2"`.
An explicit name takes precedence. In-memory presets still require a name,
and invalid explicit names or kinds remain errors.

The Python preset reader is `arpeg.profile.parse_profile(text, path=None)`;
pass the source `Path` when reading a file. Rust's `parse_profile(text, path)`
accepts `Some(path)` for file content or `None` for in-memory content. Both live
hosts supply the profile path, as do Rust's validation and file-rendering commands.
The resulting Python score retains its resolved name and kind when serialized.

For example, `up.toml` can contain:

```toml
title = "Up"

[body]
selection = { kind = "ascending" }
rhythm = { kind = "grid", step = "1/4 beat" }
```

## Development

Building the Rust MIDI host on Linux requires ALSA development headers and
`pkg-config` (`libasound2-dev` and `pkg-config` on Debian/Ubuntu). ALSA is also
needed at runtime. Windows and macOS builds use their system MIDI services.
The pure event core does not open devices.

GitHub Actions builds and tests both implementations on Linux, Windows, and
macOS, including Python installation with the MIDI extra. These tests need no
physical MIDI device and do not establish hardware latency or USB-driver behavior.

```sh
uv sync
uv run pytest
uv run ruff check .
uv run ruff format --check .
uv run ty check src
cargo test --workspace
```
