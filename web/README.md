# Browser instrument

This is the first playable arpeg front end. It runs the existing Rust MIDI
player in WebAssembly and sends its output to a small Web Audio oscillator
synth. Python remains the reference implementation. There is no JavaScript
arpeggiator and no backend or JavaScript package dependency.

## Build and play

Install Node.js, Rust 1.87 or newer, the `wasm32-unknown-unknown` target, and
`wasm-bindgen-cli` **0.2.129**, matching the pinned Rust dependency. See the
[WebAssembly build instructions](../crates/arpeg-wasm/README.md).

From the repository root:

```sh
node web/build.mjs
python3 -m http.server 8001 --bind 127.0.0.1 --directory dist/web
```

On Windows, use `py -m http.server 8001 --bind 127.0.0.1 --directory dist/web` for
the second command. Open `http://127.0.0.1:8001`, click **Start audio**, and hold
notes with the on-screen keyboard or A W S E D F T G Y H U J K. The computer keys cover
C4–C5; the on-screen keyboard covers C3–C5 and supports multiple touches.
Focused piano buttons also play with Enter or Space. Typing shortcuts leave
form controls and browser modifier shortcuts alone.

The explicit IPv4 address makes all browsers reach the same server. If another
program already uses that address and port, choose an unused port and update
the URL accordingly; do not stop unrelated services. `localhost` can reach
different IPv4 and IPv6 listeners when both exist on the same port.

`dist/web` is a self-contained static site suitable for an HTTP/HTTPS host.
Opening the HTML directly as a local file is not supported. Published releases
include it as the `arpeg-web-instrument` artifact.

## Controls

- **Note order** offers up, down, up/down, inside out, outside in, as played, and
  shuffle. Shuffle uses seed 42 for repeatable experiments.
- **Tempo**, **step**, **gate**, **transpose**, and **velocity** shape the phrase.
  Quarter notes are one beat; sixteenth notes are one quarter of a beat. Gate is
  a percentage of each step. Velocity affects newly pressed notes.
- **Latch** retains a released chord and replaces it when a new overlapping
  group of keys is pressed. **Clear notes** empties the chord.
- **Pitch bend** spans two semitones in either direction; its slider holds its
  position until changed.
- **Waveform**, **volume**, and **ADSR** control the built-in synth. New notes
  use the current waveform and envelope; volume and bend affect ongoing notes.

Changing order, step, or latch restarts the player with keys currently held.
Tempo, gate, and transpose use the existing live control API. Stop cancels
queued sound and releases input keys; a latched chord survives Stop until
cleared or replaced. Start resumes the paused transport. Leaving the page or
hiding the tab stops playback; returning requires Start. Window focus is not a
playback requirement, since embedded browsers can report the page as unfocused
even while its controls are in use. An AudioContext that does not enter the
running state produces a visible startup error.
Capture editing, sample playback, and external MIDI are not included here.

## Timing

The player receives monotonic microseconds from `AudioContext.currentTime`.
A 5 ms host poll collects due MIDI and lifecycle events. Exact lifecycle beats
are converted to audio times with a **40 ms presentation delay**, so ordinary
polling variation does not change note spacing. The player is never advanced
ahead of live input. Pending events at a tempo change are drained under the old
tempo before switching the conversion rate. Pitch-bend entry messages are
scheduled with their attacks; live bend changes use the delayed input time.

This adds 40 ms of input latency plus the browser/device audio latency. Stalls
longer than that delay can still make notes late: past times are clamped to the
current audio time, without claiming perfect timing under browser suspension.
Buffer exhaustion stops the instrument with a visible error. Stop, Clear,
preset changes, and page hiding cancel already scheduled voices. Zero gates
remain zero-length notes, and early releases shorten attack/decay correctly.
The Playing display follows lifecycle times; it describes MIDI ownership, not
the synth's audible release tail.

## Verification

After building:

```sh
node --test crates/arpeg-wasm/tests/browser.mjs web/test/music.mjs web/test/synth.mjs
```

Tests use the actual generated WASM player to check offered orders, latch,
irregular polling, tempo changes, note display timing, and bend ordering. Unit
tests also cover shared keyboard ownership and envelope release levels. They
do not exercise a real browser audio device or establish measured input latency.
