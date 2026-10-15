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

## Keyboard-to-text

Open `text.html` from the same built static site. Click **Start**, then hold
printable typing keys in the focused input area. The Rust item engine selects
opaque string handles without constructing MIDI notes, messages, or an audio
context. The browser maps emitted handles back to the original strings.

Ascending follows entry order into the bank; reverse, back-and-forth, inside-out,
and outside-in use those same ranks. Latch replaces the bank when a new group
begins after all keys have been released. Identical characters from different
physical keys retain separate input identities. Browser key auto-repeat is
ignored. Each occurrence start appends text; ends do not remove it.

Before starting, choose straight timing, 2:1 swing, or 3:1 swing. Swing stays
fixed during playback; starts move first, then the 80% gate follows each new
interval. This small example uses 120 BPM and a quarter-beat average step. Leaving
the input area stops playback, releases input state, and clears the latched bank;
returning requires Start. Clear text erases only the displayed result. Characters
are handled within this page, with no global keyboard capture or typing into
other applications. IME composition is not captured by this key-down/key-up demo.
Browser timers can delay displayed text during stalls or tab suspension.

The `TokenPlayer` constructor takes exact-string accent and gate arrays after
its swing argument. For example, pass `['1', '0', '1/2']` and `['1', '1/2']`
to mute every second opportunity in a three-step accent loop and alternate gate
multipliers. Muted opportunities do not consume the next text selection; other
accent values do not change character case or content. The text page supplies
neutral `['1']` arrays. Musical presets use the same core patterns and realize
nonzero accents as velocity multipliers.

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
node --test crates/arpeg-wasm/tests/browser.mjs web/test/*.mjs
```

Tests use the actual generated WASM player to check offered orders, latch,
irregular polling, tempo changes, note display timing, and bend ordering. Unit
tests also cover shared keyboard ownership and envelope release levels. They
do not exercise a real browser audio device or establish measured input latency.
