# Remaining arpeggiator work

Reviewed 2026-11-22. See the [README](../README.md) for supported features and
playable presets, [performance verification](../doc/performance.md) for recorded
checks, and the [hardware plan](hardware.md) for the Raspberry Pi appliance.

## Browser timing verification

Measure input-to-audio latency and scheduling jitter in an actual browser with
an audio device. Record the browser, platform, audio configuration, and test
load separately from the [WebAssembly and Node results](../doc/performance.md#webassembly-and-javascript-verification-2026-10-27).

## Live marked-sample playback

Connect arpeg's live selection to [enge's prepared region playback](../doc/marked-samples.md).
Define how the audio host admits selections and schedules them on frame
boundaries. Reuse prepared assets and the existing sampler, and keep the
standalone MIDI core independent of audio.

- Preserve exact source-order reconstruction through the live path. Verify it
  separately from reordered or repeated playback with WAV regressions of at
  least one second at 48 kHz.
- Define how live gates drive enge's whole-region or gate-tail behavior, and when
  to apply its seam fade. Looped sustain, pitch/time processing, and advanced
  region selectors require explicit playback policies and separate regressions.
- Define a region-to-key map for external MIDI samplers and diagnose an unmapped
  destination before playback.

## Phrase conversion and captured-bank extensions

Add fixed-note/drum material independent of live input and conversion of a
recorded phrase against a separately played chord. If the conversion uses
automatic chord recognition, specify how it resolves ambiguity.

Define source-note and gesture ownership before another pitch-conversion mode.
Preserve the original event ledger, source identity, entry state, release
velocity, and note-local expression. Reordering notes must not reverse their
gestures or borrow the previous output note's controller state.

Additional captured-bank work includes:

- Chord grouping for history/phrase inversion and voicing. Time-spanning notes
  do not already constitute an unambiguous simultaneous chord.
- Captured-gesture strums and multiple captured arp lanes. The current strum/lane
  contract is held/latched only, with shared expression within each channel.
- Rhythms beyond the captured-bank grid contract and other selectors currently
  rejected by those banks. Reuse the existing scheduler and exact traces rather
  than ignoring unsupported settings.
- Explicit bar/count capture boundaries, bounded musical-time history, capture
  editing, and any silence-based commit policy. A silence delimiter needs a stated
  duration and a hard recording limit; breath noise or a stuck note cannot leave
  an unbounded capture open.

For additional gesture playback modes, specify reversal, crop, hold, and any
post-release tail ownership separately. Preserve the existing capture eligibility
boundary, entry state, event order during gate fitting, and treatment of controls
in the following gap. An emitted tail needs an independently addressable
destination.

## Live timing, conditions, and algorithmic extensions

Remaining timing work includes live swing/groove modulation and timing templates
for custom patterns. Specify the boundary where changes take effect, preservation
of rhythm phase, and treatment of queued repeats, strums, gates and ties. Do not
insert events into already emitted time. Specify concrete behavior and traces
for any extensions to pattern/pass conditions.

Random grammar alternatives and recursive branch expansion remain deferred.
External algorithms must prepare or record bounded decisions outside event/audio
callbacks. There is no unrestricted callback execution or `eval` field. Live
provider execution needs an explicit capacity and timing contract first.

Extend the limited file renderer only when requested. It must continue to reject
unsupported live features instead of discarding them.

## Expression, allocation, and continuation

Independent overlapping per-note gestures need explicit channel allocation,
release-tail reservations, and a channel-exhaustion policy. MPE and MIDI 2.0 are
separate extensions; a UMP container alone does not establish per-note capability.

Preserve high-resolution controls and atomic groups such as paired controllers
and parameter-selection/data transactions. Keep destination/global events on
their declared timeline instead of replaying configuration at every note-on.
Unknown observations remain in the ledger unless explicit passthrough permits
emission.

Never reset a shared channel while another owned note needs its state. Sustain
pedal ownership and pedal-held lifetimes are distinct from arp latch and key-up;
a generated note-off does not guarantee silence. Repeated same-key sources keep
distinct identities and a documented pairing/handoff policy.

Define explicit typed operations and bounds for expression-source mixing,
such as multiplying breath or adding bend offsets.

Extend the [enveLFO bridge](../doc/envelfo.md) to map Patch outputs into arpeg's
existing density, gate, and selection ports, and add serialized combined Rust
host snapshots. Feedback needs an explicit delay and bounded event budget. Preserve
transport observations, random state, pending occurrences, controller state,
and delivery ownership at a consistent snapshot boundary. Hardware restoration
requires reconciliation; internal state cannot rewind a synthesizer.

## Performance and release acceptance

Complete the physical loopback, controller/synth, unplug/replug, clock-loss, and
clean-install checks in [doc/performance.md](../doc/performance.md). Existing
macOS virtual-endpoint results do not qualify physical interfaces or
Linux/Windows/Pi hardware.

Hosts currently timestamp callback arrival and send due batches immediately.
Driver timestamps, future output scheduling, pacing, and bounded controller
coalescing remain possible future work. Add them only against a measured need
and explicit quality contract. Coalescing must preserve initial/final and
discontinuous changes, with declared value/time error bounds. Releases, pedal
transitions and atomic message groups are not redundant controller samples.

Keep release obligations on capacity exhaustion: refuse new admission, report
the failure, and release only owned output. Do not evict a still-needed breath
stream to admit another note. Measure driver and physical delivery separately
from event-core timing, and record protocol/profile limits with release evidence.

## Dependency updates

The lockfiles still select the existing dependencies. These items come up when
updating them or preparing a release from fresh source dependencies.

- **MIDI hosts.** Migrate the Python and Rust hosts from meedee to middy.
  Recheck MIDI 1 decoding, transport-loss reporting, clock and expression
  handling, and release cleanup through the new APIs. Qualify actual delivery
  on the new transport. MIDI 2.0 and MPE still need arpeg-side behavior and
  device qualification.
- **uFor.** Update Python tuning preparation for field-based tuning sources
  and the current expression API. Align the Rust/WebAssembly profile parser,
  presets, and shared tuning traces. The old table field is `values`; the
  current uFor fields are `ratios` and `frequencies`. Arpeg also imports
  `MAX_FRACTION_BITS`, which current uFor has removed.
- **enveLFO.** Move the optional Rust bridge from its pinned `envelfo-core`
  package to upstream's consolidated `engelfo` crate when updating the bridge.
  Verify Python and Rust scheduling and continuation together after the update,
  including a device-free WebAssembly build.

## Additional work beyond the prompt

None.
