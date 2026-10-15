# Connecting arpeg and enveLFO

Install the Python extra with `uv sync --extra envelfo` in this checkout, or
`pip install 'arpeg[envelfo]'` for a published package. The optional module is
`arpeg.envelfo`; ordinary arpeg imports do not import the enveLFO runtime.
Rust hosts use the separate `arpeg-envelfo` crate. Neither event core depends
on the other. This integration has no NumPy, audio engine, Python bindings,
MIDI device dependency, or new executable. The Rust MIDI player is used only
as a device-free development dependency in its conformance tests.

## Contract and scheduling phases

The host owns time, tempo/transport observations, MIDI delivery, and observation
cadence. A bridge connects one arpeg `hit` output to a Patch-owned Envelope's
`start` command, and maps named Patch outputs to breath, bend, or pressure.
Other arpeg events do nothing. Multiple same-time hits retain their input order
and receive distinct Envelope command ordinals. The bridge does not route
note-end to release automatically: a rhythmic hit Envelope is not a voice gate.

For each scheduled timestamp:

1. Deliver input and transport changes using arpeg's existing ordering.
2. Advance arpeg to that timestamp and collect its MIDI messages and port events.
3. Deliver its hit events to `bridge.receive` at their exact **host seconds**.
4. Call `bridge.observe` once after all same-time hits, supplying the explicit
   next-delivery time for Threshold edges.
5. Pass returned expression controls to the MIDI player's existing `control`
   method at that same timestamp, after the note messages from step 2.

Do not substitute an event's beat position for host seconds. Supply Python's
`tempo_map` or Rust's per-child `ClockMapping` when the Patch has beat-clock
children. Apply the same accepted tempo/transport observations to both mappings.
The default seconds clock keeps advancing while arpeg is stopped; a beat clock
follows its host's elapsed-beat mapping. A transport seek relocates arpeg but
does not implicitly reset every enveLFO child. Send explicit Patch commands when
a particular Envelope or LFO should reset, release, or seek.

**Ordering decision to revisit:** the first integration deliberately sends
newly hit-triggered expression **after the note-on, at the same timestamp**.
The note's entry bundle can therefore contain the previous expression value.
This preserves arpeg's current delivery behavior, but may matter for instruments
that latch expression at note-on. Reconsider pre-note-on observation after
separating note scheduling from MIDI realization; do not claim that the current
bridge provides it. This decision is covered by the shared wire trace.

A host may wake irregularly, but must still process every scheduled boundary
chronologically. Advancing the player far ahead and then feeding old hits back
at the wake-up time loses their timing and cannot be repaired by this bridge.
Merge arpeg deadlines, enveLFO event/next-delivery deadlines, external input,
and the chosen expression-observation schedule. This is an explicit host
composition, not a second autonomous scheduler or a fixed-rate polling loop.
Drain port batches after operations and honor `exhausted`; never silently drop
an over-capacity batch. Feedback, density, gate, and selection mappings are not
implemented here. There is no same-time feedback loop.

## Python

Prepare the independent LFO and hit-triggered Envelope as one standalone Patch:

```python
from fractions import Fraction

from envelfo import patch
from ufor import envelfo
from ufor.segments import Segment

from arpeg.envelfo import EnveLFOBridge

definition = patch.PreparedPatch(
    envelfos={
        "wave": envelfo.LFO(rate=1, shape="triangle", center=0.5, depth=0.5),
        "attack": envelfo.Envelope(
            start="event",
            retrigger="reset",
            segments=[Segment(duration="250 ms", to=1)],
        ),
    },
    outputs={"breath": "wave", "accent": "attack"},
)
bridge = EnveLFOBridge(
    definition=definition,
    state=patch.initial_patch(definition, Fraction(0)),
    hit_target="attack",
    expression={"breath": "breath", "pressure": "accent"},
)
```

Configure the player's profile with
`expression = { lanes = { breath = "envelfo", pressure = "envelfo" } }`.
Bend retains its current or recorded ownership. Input controllers continue to
be captured; an enveLFO-owned lane does not overwrite the captured source data.
The player rejects controls for lanes not explicitly owned by enveLFO.

At a host-selected boundary, with `player`, `at_ns`, and `next_delivery` supplied
by that host:

```python
messages = player.advance(at_ns)
at = Fraction(at_ns, 1_000_000_000)  # relative to the Patch's chosen host epoch
batch = player.take_events()
if batch.exhausted:
    raise RuntimeError("arpeg output capacity exhausted")
events = []
for event in batch.events:
    events.extend(bridge.receive(at, event))
observation = bridge.observe(at, next_delivery=next_delivery)
events.extend(observation.events)
for command in observation.controls:
    messages.extend(player.control(at_ns, command))
```

The bridge returns all named floating-point values as well as rational
`ArpeggiatorControl` samples. Python converts the signal's decimal representation
to a Fraction; Rust approximates the floating signal with its existing rational
type. Arpeg's existing normalized-range checks and MIDI quantization still apply.
An out-of-range sample fails before committing the bridge observation.
Host event times remain exact rationals, never floating-point approximations.

Save `player.model_dump_json()` and `bridge.model_dump_json()` together at the
same phase boundary. Restore with their respective `model_validate_json`
methods. The bridge includes Patch definitions, child state, pending deliveries,
and retained values. Resupply the same host clock mapping after restoration.
Any messages or returned events already collected but not yet delivered belong
to the host and must also be saved or drained consistently.

## Rust

Build a normalized `envelfo_core::patch::Patch` with the same `wave` LFO,
`attack` Envelope, and `breath`/`accent` outputs. Set Envelope `start` to
`Start::Event` and `retrigger` to `Retrigger::Initial`. Then initialize:

```rust,ignore
use arpeg_core::ports::InputPort;
use arpeg_envelfo::EnveLFOBridge;
use envelfo_core::{patch::Seconds, time::Time};

let state = definition.initial(Time::ZERO, "patch", 0, None, &Seconds)?;
let mut bridge = EnveLFOBridge::new(
    definition,
    state,
    "attack".into(),
    vec![
        (InputPort::Breath, "breath".into()),
        (InputPort::Pressure, "accent".into()),
    ],
)?;

let mut messages = player.advance(at_us)?;
let batch = player.take_events();
assert!(!batch.exhausted);
let at = Time::new(at_us, 1_000_000)?;
for event in &batch.events {
    let events = bridge.receive(at, event, &Seconds)?;
    // Deliver or retain returned enveLFO events in the host.
}
let observation = bridge.observe(at, next_delivery, &Seconds)?;
for (port, value) in observation.controls {
    messages.extend(player.control(at_us, port, value)?);
}
```

Rust bridge continuation uses `Clone`, including pending Patch deliveries.
There is no serialized Rust bridge/player snapshot or cross-language snapshot
interchange in this slice. The host must preserve its arpeg and delivery state
alongside the bridge using its own existing continuation mechanism.

The complete normalized Rust construction is in
[the composition tests](../crates/arpeg-envelfo/tests/composition.rs).
Run `cargo test -p arpeg-envelfo` without Python or MIDI devices. Run
`uv run pytest test/test_envelfo.py` for Python. Both consume
[one shared trace](../conformance/envelfo-composition.toml), covering expressive
controllers between notes, tempo changes, stop/start, seek, and final release.

## Additional work beyond the prompt

None.
