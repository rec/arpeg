# arpeg in WebAssembly

This crate exports the existing `arpeg-midi` profile parser and device-independent
MIDI player to JavaScript. Selection, rhythm, probability, expression, capture,
and transport use the same Rust implementation as the native executable. Python
remains the reference implementation. There is no JavaScript arpeggiator.

## Build

From the repository root, with Rust 1.87 or newer:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
cargo build -p arpeg-wasm --no-default-features --target wasm32-unknown-unknown --release --locked
wasm-bindgen --target web --out-dir dist/wasm target/wasm32-unknown-unknown/release/arpeg_wasm.wasm
```

The binding generator must match the crate's pinned `wasm-bindgen` version.
`dist/wasm/` contains the WebAssembly module, JavaScript loader, and TypeScript
declarations. The native MIDI drivers are disabled for this build. For native
builds, the existing `device-host` feature remains enabled by default.

## JavaScript API

Serve the generated files with the webpage, then import the loader:

```javascript
import init, { MidiPlayer } from './wasm/arpeg_wasm.js';

await init();
const profile = await (await fetch('./profiles/up.toml')).text();
const player = new MidiPlayer(profile, 'internal', 120, 500_000n);

player.accept(0n, new Uint8Array([0x90, 60, 100]), 'both');
player.accept(0n, new Uint8Array([0x90, 64, 90]), 'both');
const messages = player.advance(0n);
// messages is an array of Uint8Array MIDI messages.
console.log(messages, player.beat, player.active);

const releases = player.stop(50_000n);
// Send releases before freeing the player.
player.free();
```

Constructor arguments are TOML profile text, clock mode (`internal` or `external`),
integer BPM, and external-clock timeout in microseconds. Input must contain one
complete MIDI message with its status byte, as supplied by Web MIDI, rather than
a partial serial stream. `accept` takes a source of `notes`, `clock`, or `both`,
matching the native player's input/clock routing.

All times are monotonic microseconds represented as JavaScript **BigInt**, so
integer timing survives the language boundary. The host chooses a time origin
and uses it consistently; for example, convert elapsed `performance.now()`
milliseconds with `BigInt(Math.round(elapsed * 1000))`.

`accept`, `advance`, `control`, `capture`, `clear`, `stop`, and `set_tempo` return an array of complete
MIDI messages to emit, in order. The `beat` property is an exact rational string;
`active` reports transport state. Invalid profiles or operations throw errors.
`clear` follows the native engine's bank restrictions. `stop` pauses and releases
owned output; `free` only disposes the object and does not send MIDI releases.

With a `bank = { kind = "phrase", publish = "step" }` profile, use
`capture(time, "record")` to begin a take, `"commit"` to replace the bank,
`"overdub"` to add it, or `"undo"` to remove the latest committed take.
Recording starts and ends immediately; committed bank edits publish at the
next grid step. Existing material plays during recording. `clear` also discards
the recording. Capturing while transport is paused is supported.
`capture_state` returns `[recording, publishedNoteCount, bankRevision]` for
captured banks and an empty array for held or latched banks.

Motion hosts use `control(time, "gate", "1/2")`,
`control(time, "density", "2/3")`, or `control(time, "transposition", "-12")`.
Transposition requires whole signed 64-bit semitones. Presets configure
`body.transposition = { semitones = 0, boundary = "drop" }`; boundary may also be
`fold` (minimal octave folding into MIDI 0–127) or `error` (stop playback on a
reported pitch error to release the last delivered note). Source identity and
recorded expression are retained. Sounding notes and pending repeats keep
their realized pitches. Values are exact rational strings. Fractional
density requires a preset seed; it replaces the preset probability. Queued
changes publish at the next step, preserve already realized gates and repeats,
survive pause, and are cancelled by Start or song-position relocation.

Selection offset uses `control(time, "selection_offset", "-1")` and preset
`body.selection_offset = { ranks = 0, boundary = "wrap" }`. It shifts each
selector result in ascending pitch/source-identity order while preserving the
selector's own progression, then applies transposition. The target supplies its
velocity and recorded expression. `rest` is the other boundary policy; it skips
out-of-bank targets while advancing selection. Whole signed 64-bit ranks publish
at the next step; sounding notes and pending repeats retain their target.

Call `take_events()` after each operation to collect
`{ events, exhausted }`. Each event is `{ at, port, index, revision }`: `at` is
an exact beat string, `port` is `step`, `hit`, or `rest`, and the counters are
BigInt values. Each step emits `step` before its `hit` or `rest`; ties emit only
`step`, and repeats share one hit event. The 4096-event buffer stops admitting
new steps when full while retaining release obligations; draining reports the
exhaustion and permits future steps. Skipped attacks are not replayed.
Expression lanes declare their source through `body.expression.lanes`, for
example `{ breath = "motion", bend = "recorded", pressure = "current" }`.
Unspecified lanes inherit `body.expression.source`. Recorded lanes require a
history or phrase bank. `control(time, "breath", "1/2")`,
`control(time, "bend", "-1/4")`, and `control(time, "pressure", "1")` update
Motion-owned lanes immediately. Breath/pressure range from 0 to 1; bend ranges
from -1 to 1 with center 0, leaving the musical interval to the synth. Values
round to the nearest MIDI integer, ties upward. Samples are held through rests
and pauses, restored before the next note-on, and never added to the input
recording. An unknown lane has no invented initial value. Controls for lanes
owned by current or recorded expression throw before changing playback state.
The webpage owns Motion sampling, event routing, and any delayed feedback.

The webpage supplies its controls, clock polling, sound or MIDI output, and any
browser permissions. This crate does not open MIDI ports, produce audio, or
provide a demo page. Returned messages are due at the supplied time; the API
does not provide a future-timestamped audio queue. Browser playback timing still
requires its own host implementation and measurement.

## Tests and release workflow

Install Node.js and the matching `wasm-bindgen-cli`, then run:

```sh
cargo test --workspace --no-default-features --target wasm32-unknown-unknown --tests --locked
node --test crates/arpeg-wasm/tests/browser.mjs
```

The Cargo runner configuration executes the existing core and portable MIDI
tests inside WebAssembly using Node.js. They reuse the same fixtures as native
tests, including seeded selections, expression, and transport. Browser binding
tests additionally check JavaScript MIDI byte arrays and input errors. Fixture
data is embedded, so the tests need no browser filesystem.
The Node test additionally loads the generated release module through its web
loader and calls the exported JavaScript API. Build `dist/wasm/` first.

The existing GitHub workflow now runs only on **published releases**, including
prereleases. It builds the native executable and tests Python and Rust on Mac,
Linux, and Windows, then separately tests and builds WebAssembly. The browser
module and generated bindings are retained as the `arpeg-webassembly` workflow
artifact. Publishing a release does not deploy a website.
