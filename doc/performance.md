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
the end of their processing batch. It includes wake-up jitter and batch work.
The current hosts generate complete batches before output-budget admission and
delivery. These probes measure generation and collection, not port acceptance.
Byte rate counts MIDI message bytes,
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

At that revision, the Python suite passed 226 tests. Native Rust tests and Clippy
with `-D warnings` passed on the CI-pinned Rust 1.87 toolchain. Installed Clippy
1.99 reported an existing `collapsible_match` warning in `rhythm.rs`.
These timing observations predate the meedee migrations and strum/lane support;
they are not measurements of the current transport or multiple-lane workloads.

## Correctness and capacity evidence

The Python/Rust dense-capture regression records 2,000 equal breath samples
at 1 kHz. Fitting the phrase to a 100 ms gate retains the samples: its first
50 ms emits 1,000 controllers. Stop emits one matching release and cancels the
remaining stream. This also demonstrates how fitting can greatly increase
controller bandwidth; preserved data does not imply a receiver can keep up.

Existing shared lifecycle tests cover stale releases, handoffs, zero gates,
repeats, pause/clear/seek cleanup, event-buffer exhaustion, and recovery after
draining. Decision output is bounded at 4,096 events per lane, with up to 16
lanes merged by the player. The shared 4,096-event lifecycle buffer reserves
every owned release; queued strum attacks also have a 4,096-entry limit. Phrase
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
Python receives raw MIDI 1 bytes through meedee and maintains independent bounded
decoders for note/shared-clock and separate-clock inputs. Decoder diagnostics,
receive drops and sequence gaps end playback. Automated checks do not
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


## meedee host migration, 2026-10-18

The Rust device host now uses the HTTPS `main` branch of meedee instead of
midir. Cargo.lock resolves meedee to `38c0cbcd8e267c8acb5c481cdd5cb56411663cdc`.
Only the device-host feature enables it. The Python host, musical engines,
file renderer, WebAssembly bindings and authored profiles are unchanged.

Host callbacks continue to timestamp arrival with `Instant`; backend timestamp
capture is disabled. Raw MIDI 1 chunks go through arpeg's existing bounded
framer. Separate note/clock inputs retain independent parser state and use each
client's own discovery snapshot. A changed clock selection during discovery
fails explicitly. meedee receive drops or callback panics terminate playback.
The existing byte budget and sent-note ownership decide output and cleanup;
failed sends are not retried. Explicit connection closes report errors, including
when musical cleanup has already failed. Rust native operations remain in-process
and can block on a native driver; Python isolation is not implicitly introduced.

The Git dependency builds bundled libremidi successfully on this Mac with Rust
1.99.0, CMake 4.4.4 and the installed C++ toolchain. Rust workspace tests pass:
135 ordinary tests, with one native check ignored. The no-default-features suite
passes 122 tests. Workspace Clippy with `-D warnings`, formatting and a release executable build
pass. Standalone download documentation includes the statically linked transport
licenses without changing the packaging tool.
The WebAssembly dependency tree excludes meedee; the local Rust installation
has no wasm32 target, so actual WebAssembly compilation remains with the
existing release-only CI gate. No CI trigger changes or workflow runs were made.

The native check requires explicit invocation:

```sh
cargo test -p arpeg-midi --test native_host --locked -- --ignored --nocapture
```

It launches a supervised native fixture with a 30-second deadline, terminating
its process group on timeout. The fixture creates only its own virtual source,
clock source and destination, discovers their CLI indices and launches the actual
`arpeg play` executable. It verifies NoteOn delivery and matching NoteOff after
quit with internal clock, shared external clock and separate external clock.
A fourth case sets the native receive budget below a NoteOn packet's size and
checks explicit transport-loss termination without producing output.
Ordinary test runs do not create endpoints or contact the MIDI service.

An agent-context attempt during this migration failed before creating endpoints,
with CoreMIDI keeper initialization error 268435459. The owner's ordinary Terminal
run passed all four cases, with the supervised result completing in 2.61 seconds. See
[the retained Terminal result](meedee-host-native-macos.txt). This establishes
virtual-only production host behavior on this Mac, not physical interface
latency, unplug/replug recovery or Linux/Windows qualification. Build dependencies
and those platform gates remain explicit; release CI is still release-only.


## Python meedee host migration, 2026-10-19

The Python host and MidiPlayer now use native immutable meedee messages instead
of Mido/RtMidi. The optional MIDI extra uses meedee from HTTPS `main`; uv.lock
records revision `2f444e311a558dc8ada9bb89530639aaa6e04c87`.
The pure musical engines remain independent of native transport.

Python transport defaults to process isolation; `--no-isolated` selects
in-process operation. Both modes preserve parent callback arrival timestamps,
with backend timestamp capture disabled. Independent bounded decoders frame note
and separate-clock inputs. Decoder diagnostics, receive drops, sequence gaps and
worker failures end playback. Cleanup releases only successfully sent owned
notes and does not retry failed sends. Native message channels use 1–16;
existing wire output and musical conformance fixtures are unchanged.

The ordinary Python suite passes 488 tests. Ruff, Python formatting, type checks
and pyupgrade pass. Native checks are explicitly selected, not part of ordinary
test collection:

```sh
uv run --frozen --extra midi pytest -q -s test/qualification/python_host.py
```

Each process mode has a 45-second supervisor and uses only test-created virtual
endpoints. The actual Python CLI delivers NoteOn and quit cleanup NoteOff with
internal, shared external and separate external clocks. A fourth case per mode
sets the native receive budget below a NoteOn packet's size, then checks explicit
loss termination without another packet. That quiet-loss case exposed stale
isolated drop reporting; the meedee revision above fixes it using the existing
forwarder's idle wake-up, without additional MIDI traffic.

The owner's ordinary Terminal run passes all eight scenarios in 8.28 seconds.
See [the retained Terminal result](meedee-python-native-macos.txt). These checks
establish virtual-only operation on this Mac. They do not establish physical
interface latency, unplug/replug behavior or Linux/Windows/Pi qualification.
Isolation adds process and communication overhead; in-process native operations
can block on drivers. Release-only CI triggers are unchanged.

## Device-free musical features, 2026-10-21

The delivery ending at `8c5200c` added strums and dedicated-channel arp lanes,
following captured-bank selectors and authored phrase templates. The arpeg
Python suite passed 612 tests; the related uFor schema work passed 1,305 tests.
Rust workspace tests passed with default features disabled, including the
shared strum/lane cases. Ruff, Python formatting, type checks, pyupgrade,
Rust formatting, device-free Clippy with `-D warnings`, and `git diff --check`
passed.

Native Rust binaries and tests also compiled with `--no-run`. A first attempt
encountered a CMake cache containing a different Cargo checkout path; compilation
succeeded with a fresh target directory. No existing build artifacts were deleted.
The verification commands included:

```sh
uv run --frozen pytest -q test
cargo test --workspace --no-default-features --locked
CARGO_TARGET_DIR=/private/tmp/arpeg-strums-native-check cargo test --workspace --locked --no-run
```

Use the explicit `test` path for local Python collection: an unrestricted run
also collected dependency checkouts under `target`, causing duplicate-module
collection errors.

No MIDI devices or virtual endpoints were opened for this delivery. Actual
WebAssembly/JavaScript execution and physical MIDI checks were not performed;
native compilation does not establish either. No release workflow was triggered.
The documentation review on 2026-10-27 does not constitute a new test run.
