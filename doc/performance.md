# Native delivery and performance verification

## What these checks cover

`tools/check_performance.py` and the Rust `check_performance` example exercise
the real MIDI players without opening MIDI ports. Each two-second workload
holds one latched note at 120 BPM with sixteenth-note steps and a 150% gate,
so successive occurrences hand off channel 1. Equal-valued CC2 messages arrive
at 1,000 or 4,000 samples per second. The recovery case deliberately stalls
the host loop for 200 ms halfway through the run.

Both probes poll with a 1 ms sleep and drain decision/lifecycle notifications.
They check matching occurrence identities, release every owned note on Stop,
and fail if an event buffer exhausts. Timing numbers are observations, not
pass/fail thresholds or a real-time guarantee.

Run the probes one at a time after builds finish:

```sh
uv run --frozen python tools/check_performance.py --controller-hz 1000
uv run --frozen python tools/check_performance.py --controller-hz 4000
uv run --frozen python tools/check_performance.py --controller-hz 1000 --stall-ms 200
cargo run -p arpeg-midi --example check_performance --release --locked
```

The Rust command runs all three cases. Release CI runs both probes on each
native platform and leaves their reports in the job logs.

`batch_p99_us` measures player processing and event collection, excluding the
poll sleep. Note lateness compares exact lifecycle beats with the wall time at
the end of their processing batch. It includes wake-up jitter and batch work;
Rust's real host sends individual messages during processing, so the batch
measurement is conservative for that host. Byte rate counts MIDI message bytes,
including repeated controllers and entry bundles. It excludes transport framing.
None of these numbers measures a MIDI driver, cable, synth, or audible latency.

## Local observations, 2026-10-13

Measured on macOS 14.5 ARM64 with Python 3.13.12 and Rust 1.87.0 in release mode.
Builds had finished before the workloads ran. Each row is one two-second run;
system load and timer behavior can change the results substantially.

| Runtime | CC2 samples/s | Injected stall | Batch p99, µs | Maximum note lateness, ms | MIDI bytes/s |
| --- | ---: | ---: | ---: | ---: | ---: |
| Rust | 1,000 | None | 141 | 2.876 | 3,066 |
| Rust | 4,000 | None | 184 | 2.281 | 12,060 |
| Rust | 1,000 | 200 ms | 200 | 208.916 | 3,067 |
| Python | 1,000 | None | 2,317 | 4.976 | 3,066 |
| Python | 4,000 | None | 2,944 | 4.003 | 12,051 |
| Python | 1,000 | 200 ms | 1,871 | 216.069 | 3,064 |

Every row produced 16 attacks and 16 matching releases, with zero owned notes
after Stop and no event-buffer exhaustion. Rust's player batches were much
shorter; Python's p99 processing batches exceeded the nominal 1 ms poll interval.
Neither runtime can preserve delivery deadlines through the injected stall.

The Python suite passes 226 tests. Native Rust tests and Clippy with `-D warnings`
pass on the CI-pinned Rust 1.87 toolchain. Installed Clippy 1.99 reports an existing
`collapsible_match` warning in `rhythm.rs`; that unrelated source was left unchanged.

## Correctness and capacity evidence

The new Python/Rust dense-capture regression records 2,000 equal breath samples
at 1 kHz. Fitting the phrase to a 100 ms gate retains the samples: its first
50 ms emits 1,000 controllers. Stop emits one matching release and cancels the
remaining stream. This also demonstrates how fitting can greatly increase
controller bandwidth; preserved data does not imply a receiver can keep up.

Existing shared lifecycle tests cover stale releases, handoffs, zero gates,
repeats, pause/clear/seek cleanup, event-buffer exhaustion, and recovery after
draining. Both 4,096-event buffers preserve owned releases on exhaustion. Phrase
tests cover the 128-take limit and recovery through undo/clear. Captured input
has a one-million-event admission limit.

Native archives retain executable permissions, playable profiles, and a matching
SHA-256 checksum. The locally built macOS executable validates a profile without
opening MIDI devices. Its dynamic dependencies are operating-system libraries;
no Python, enge, NumPy, or external audio engine is linked. Hosted native jobs
perform the same executable validation before retaining their archives. The
publication job waits for all native and WebAssembly checks before attaching
downloads to the release.

## Remaining limitations and hardware checks

The Python and Rust device hosts bound incoming queues at 4096 packets and
1 MiB of MIDI payload, with a 64 KiB packet/assembled SysEx limit. Limits are
configurable, shared across note and clock inputs, and tested at capacity
boundaries and with concurrent producers. Console input is also bounded.
Overflow is fatal: release owned output, report the failure, and end the play
session. Fake-port Python tests verify cleanup, including a release produced
just before overflow. Rust tests cover fragmented SysEx and capacity reservation
before payload allocation. There is no queue-age cutoff.

Native hosts now offer an optional output byte budget, disabled by default.
`--output-bytes-per-second` sets the refill rate and `--output-burst-bytes` sets
the allowance (default 64 bytes). Whole player-produced batches are admitted
before any sends, retaining order and controller samples. Overload refuses the
batch, reports an error, and ends playback. Cleanup bypasses the budget and uses
a ledger of notes accepted by the port, rather than the player's potentially
newer generated note. Python/Rust tests cover exact refill boundaries, burst
caps, failed sends, release velocity, and refused note-entry groups; a Python
fake-port test verifies cleanup when the player has already selected a new note.

Hosts timestamp callbacks, use 1 ms polling, and send due output immediately;
they do not yet use driver timestamps or future output scheduling. There is no
controller coalescer or pacing queue. A long stall can produce a late burst
within capacity, or end a session when its configured output budget is exceeded.
Sustained input overload also ends the session.
The input limits cover arpeg's retained payload, not MIDI-library or driver buffers.
The output budget estimates capacity, without measuring driver queue occupancy.
Python receives already assembled Mido messages. Automated checks do not
establish a latency ceiling or prove device operation under a MIDI storm.

Hardware verification is pending. When the devices are available:

1. Use a MIDI loopback destination and timestamp the returned notes/controllers
   to measure actual delivery order and timing, including dense breath.
2. Play the wind-controller profile into the intended synth; check inherited
   breath, delayed attack, bend, and release articulation.
3. Check Stop, external-clock loss/reacquisition, clear, and a disconnect while
   notes are sounding. Software can attempt cleanup but cannot deliver a release
   through an already disconnected output.
4. Repeat native installation/validation on systems without a Python/audio
   development environment. Linux/Windows hardware remains separate evidence
   from their hosted build and software checks.
