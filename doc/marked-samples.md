# Marked-sample playback

arpeg's Python and Rust marker helpers turn ordered frame markers into
identified source regions. Regions use integer source-frame boundaries. An
exhaustive set starts at frame zero and the last region ends at the decoded
asset boundary. Selection keys order the regions independently of acoustic
pitch, which can remain unknown for material such as drums, noise, and speech.
Both helpers use the same [region fixture](../conformance/marked-sample.json).

enge's `RegionPlayer` plays a supplied order of regions from prepared decoded
audio in successive blocks through its NumPy or Rust sampler. The caller
selects the channels explicitly and treats source audio as immutable. Adjacent
regions from the same asset share a continuous source cursor. One traversal in
source order at the original rate and timing, with whole-region tails and no
seam fade, reproduces the selected decoded channels frame for frame. That path
uses no extra envelope, normalization, resampling, or time stretching.

For reordered and repeated regions, the player can use the whole region or stop
at its authored gate end. An optional seam fade applies where the source cursor
jumps. The enge suite has one-second 48 kHz WAV regressions for source-order
identity, reordered and repeated regions, and seam behavior.
[enge playback documentation](https://github.com/rec/enge/blob/92bd3b754995225609fbab8cc0b1005e801b66cc/README.md)
and [region tests](https://github.com/rec/enge/blob/92bd3b754995225609fbab8cc0b1005e801b66cc/test/test_sample_regions.py)
describe that contract.

Live integration work is tracked in the [arpeggiator plan](../plan/arpeggiator.md#live-marked-sample-playback).
