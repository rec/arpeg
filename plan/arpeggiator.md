# Remaining arpeggiator work

Reviewed 2026-10-27. This plan contains unfinished work and its acceptance
contracts. Completed feature descriptions and playable presets are in
[README.md](../README.md); host integration is in
[doc/envelfo.md](../doc/envelfo.md), and verification evidence and hardware gaps
are in [doc/performance.md](../doc/performance.md). The separate
[hardware plan](hardware.md) covers the proposed Raspberry Pi appliance.

The runtimes, standalone MIDI host, external clock, expressive capture, authored
phrase templates, strums, and dedicated-channel lanes already exist. The original
project-creation handoff and completed implementation slices have been removed.
Remaining items below are separate implementation choices, not authorization to
start them all.

## WebAssembly and browser verification

Execute the WebAssembly and JavaScript binding tests for the recent musical
features, including strums, channel metadata, independent lanes, and cleanup.
Native Rust tests of the portable player do not establish execution inside
WebAssembly. Use the instructions in
[crates/arpeg-wasm/README.md](../crates/arpeg-wasm/README.md) and
[web/README.md](../web/README.md).

Measure browser/audio timing separately from native event scheduling. Capture
editing, sample playback, and external MIDI remain outside the browser front end.

## Live marked-sample playback

Frame markers already lower to identified source regions, and enge supplies
sample realization. Connect live selection to that existing playback path.
Do not add another asset loader, private resampler, or audio dependency to the
standalone MIDI core.

Preserve these acceptance contracts:

- Assets are immutable. Regions use integer source-frame boundaries and explicit
  channel selection. Selection keys describe traversal independently of acoustic
  pitch; unknown pitch stays unknown for noise, drums, and speech.
- Exhaustive regions begin at frame zero and cover the decoded asset. Ascending
  source-order selection, original timing and rate, and one traversal must
  reproduce the decoded source stream exactly at the same sample rate.
- Identity playback keeps a continuous cursor through adjacent regions and
  bypasses extra envelopes, fades, normalization, resampling, and time stretching.
- Reordered/repeated regions need explicit seam, tail, one-shot/gated/looped
  sustain, and any pitch/time-processing policies. They need listening regressions
  and do not inherit the identity guarantee.
- External MIDI samplers require an explicit region-to-key map. Diagnose an
  unmapped audio destination before playback.

Audio regressions must write WAV at 48,000 samples per second for at least one
second. Test exhaustive reconstruction and reordered/repeated playback separately.
Advanced selectors for sample regions remain unsupported.

## Phrase conversion and captured-bank extensions

Authored rank substitution and literal interval templates are complete. Remaining
phrase work includes fixed-note/drum material independent of live input and
conversion of a recorded phrase against a separately played chord. Automatic
chord recognition needs explicit ambiguity rules; it is not required for literal
interval templates and must not be introduced implicitly.

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

For a sequential phrase, cells run from one onset to the next or to the phrase
end. Keep unowned post-release controls in the following gap and use their state
for the next note's entry. Emit them as an old note's tail only under an explicit
ownership policy and an independently addressable destination. Never apply them
to whichever rearranged note happens to be sounding.

Completed gestures become eligible after their declared end, including capture
tails. Original timing, gate fitting, deliberate gesture reversal, and future
crop/hold policies are distinct operations. Fitting must preserve event order;
zero-length source gates must not cause division by zero. Keep unknown initial
controller state unknown rather than inventing zero or a future breath attack.

## Live timing, conditions, and algorithmic extensions

Remaining timing work includes live swing/groove modulation and timing templates
for custom patterns. Specify the boundary where changes take effect, preservation
of rhythm phase, and treatment of queued repeats, strums, gates and ties. Do not
insert events into already emitted time. Ordinary pattern/pass conditions are
implemented; richer conditions need their own concrete behavior and traces.

Random grammar alternatives and recursive branch expansion remain deferred.
External algorithms must prepare or record bounded decisions outside event/audio
callbacks. There is no unrestricted callback execution or `eval` field. Live
provider execution needs an explicit capacity and timing contract first.

Extend the limited file renderer only when requested. It must continue to reject
unsupported live features instead of discarding them.

## Expression, allocation, and continuation

Live players currently use one expression stream per dedicated lane channel.
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

Expression-source mixing remains unfinished. Recorded, live, and enveLFO sources
currently have one owner per expression lane. Combining them requires explicit
typed operations and bounds, such as multiplying breath or adding bend offsets.

The optional enveLFO bridge already supports scheduled pre-note expression.
Remaining integration work includes feedback/density/gate/selection mappings
and serialized combined Rust host snapshots. Feedback must have an explicit
delay and bounded event budget, never an unbounded same-timestamp loop. Preserve
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

## Additional work beyond the prompt

None.
