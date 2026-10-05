# Deterministic random choices, version 1

The Python reference and Rust core use `draw_below` to draw an integer from
`0` through `bound - 1`, where `bound` is positive. No process-global random
generator participates. The vectors in `chance-walk.json` use seed `42`, profile
name `weighted-walk`, bank revision `3`, and decisions `0` through `11`.
The probability bound is `3`; the walk bound is `5`.

For each attempt, encode this string as UTF-8, with integers in ordinary decimal
notation and the name length measured in UTF-8 bytes:

```text
arpeg-v1:{seed}:{name_length}:{name}:{lane}:{revision}:{decision}:{retry}:{chunk}
```

Start `retry` at zero. For each 64-bit chunk, start `word` at
`0xcbf29ce484222325`. For every byte, set
`word = (word XOR byte) * 0x100000001b3`, truncating to 64 bits. Then apply:

```text
word = (word XOR (word >> 30)) * 0xbf58476d1ce4e5b9
word = (word XOR (word >> 27)) * 0x94d049bb133111eb
word = word XOR (word >> 31)
```

Truncate each multiplication to 64 bits. Let `bits` be the bit length of
`bound - 1`. Assemble enough chunks, starting at chunk zero with the least
significant 64 bits, and retain the lowest `bits` bits. Return that value if
it is less than `bound`; otherwise increment `retry` and try again. A bound
of one returns zero. Python supports arbitrary integer bounds; the native core
supports bounds through `u64::MAX` and therefore uses at most one chunk.

## Live decision inputs

- `name` is the portable profile name. Renaming a profile changes its choices.
- `revision` starts at zero and increments once per bank edit. Held onsets and
  releases edit the bank. Latched onsets that change membership edit the bank;
  key releases do not. Clearing or stopping a nonempty bank also increments it.
- Lane `probability` uses a counter incremented once per nonempty, eligible hit,
  including rejected hits and probabilities zero and one. Masked steps, rests,
  ties, and empty banks do not increment it. Draw below the reduced rational's
  denominator and accept values below its numerator.
- Lane `walk` uses a separate counter incremented once per admitted walk
  selection, including initial or reset lowest-note selections. When a move
  is needed, draw below the sum of weights and select the first cumulative
  weight exceeding the draw.
- Lane `shuffle` has a separate counter incremented once per draw, including
  draws with bound one. Begin each fresh permutation with ascending pitch/source
  identity order. Apply Fisher-Yates from index `size - 1` down to `1`: draw
  below `index + 1` and swap those positions. With `no_repeat`, if the first
  identity equals the previous selection and the size exceeds one, draw below
  `size - 1`, add one, and swap that position with the first. This gives a uniform
  permutation conditional on a different first identity. Repeats within one hit
  reuse the selection.
  For preserved chord edits, remove missing identities and count surviving
  identities before the cursor to update its position. Add new identities in
  ascending pitch/source order: draw below `size - position + 1` and insert at
  `position + draw`. Use the current bank revision for all these draws. When the
  queue ends, `once` rewinds it and `cycle` creates a fresh permutation.
- Lane `choice` increments a separate counter once per admitted selection,
  including single-note draws. Order the bank by ascending pitch/source identity.
  Assign positive u32 weights by rank, extending with ones or repeating the list
  as configured. Ignore weights beyond the bank size. With `no_repeat`, exclude
  the previous identity if the bank has multiple notes, keeping each remaining
  note's original rank weight. Draw below the sum of eligible weights and choose
  the first cumulative weight exceeding the draw. Repeats share one selection;
  chance rejections, rests, ties, masked steps, and empty banks consume no draw.
- Counters survive bank edits, retriggers, clear, and stop. Retrigger resets the
  selected identity; it does not reset the random sequence. Saved Python state
  includes the revision, counters, selected identity, and previous rank.
  Shuffle snapshots also include the order, cursor, and revision used to update
  that order. Resets discard the order but retain its random counter.
  Choice snapshots retain the previous identity and counter; resets forget the
  identity while preserving its counter.

The native profile uses signed 64-bit seeds and rational components and
unsigned 64-bit weight totals. Shared profiles must stay within those ranges.
