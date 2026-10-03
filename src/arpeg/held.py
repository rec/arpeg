"""Deterministic held-note arpeggiation on an exact local beat grid."""

from fractions import Fraction
from math import ceil

from ufor.arpeggiator import ArpeggiatorScore, Ascending, Grid, HeldBank
from ufor.arpeggiator_capture import CapturedPhrase, Occurrence, SourceNote
from ufor.base import Identifier
from ufor.control import Clock, TempoMap


def render_held(
    profile: ArpeggiatorScore,
    phrase: CapturedPhrase,
    tempo: TempoMap,
    through: Fraction,
    destination: Identifier,
) -> list[Occurrence]:
    """Render the supported held/ascending/grid profile before ``through``."""
    body = profile.body
    if not isinstance(body.bank, HeldBank):
        raise ValueError("held rendering requires a held bank")
    if not isinstance(body.selection, Ascending) or body.selection.key != "pitch":
        raise ValueError("held rendering requires ascending pitch selection")
    if body.selection.repeats != 1:
        raise ValueError("held rendering does not support repeated selections")
    if not isinstance(body.rhythm, Grid):
        raise ValueError("held rendering requires grid rhythm")
    if through < 0:
        raise ValueError("render horizon must be nonnegative")

    step = Fraction(body.rhythm.step.removesuffix(" beat"))
    note_times = [
        (
            note,
            tempo.elapsed_beats(
                Fraction(0),
                Fraction(
                    note.onset_tick * phrase.timebase.rate.denominator,
                    phrase.timebase.rate.numerator,
                ),
            ),
            tempo.elapsed_beats(
                Fraction(0),
                Fraction(
                    note.gate_end_tick * phrase.timebase.rate.denominator,
                    phrase.timebase.rate.numerator,
                ),
            ),
        )
        for note in phrase.notes
    ]
    if any(n.key is None for n, _, _ in note_times):
        raise ValueError("held pitch selection requires keyed notes")

    occurrences: list[Occurrence] = []
    previous_bank: set[str] = set()
    previous_key: tuple[int, str] | None = None
    revision = 0
    for index in range(ceil(through / step)):
        at = index * step
        active = [n for n, start, end in note_times if start <= at < end]
        bank = {n.note_id for n in active}
        if bank != previous_bank:
            revision += 1
            previous_bank = bank
        if not active:
            previous_key = None
            continue
        ordered = sorted(active, key=_selection_key)
        note = next(
            (
                n
                for n in ordered
                if previous_key is None or _selection_key(n) > previous_key
            ),
            ordered[0],
        )
        previous_key = _selection_key(note)
        gate_end = at + step * body.gate
        for boundary in sorted(
            {
                t
                for _, start, end in note_times
                for t in (start, end)
                if at < t < gate_end
            }
        ):
            if not any(start <= boundary < end for _, start, end in note_times):
                gate_end = boundary
                break
        occurrences.append(
            Occurrence(
                source_capture=phrase.capture_id,
                source_note=note.note_id,
                bank_revision=revision,
                decision=len(occurrences),
                destination=destination,
                trigger_id=f"arp-{len(occurrences)}",
                clock=Clock.beats,
                onset=at,
                gate_end=gate_end,
            )
        )
    return occurrences


def _selection_key(note: SourceNote) -> tuple[int, str]:
    assert note.key is not None
    return note.key, note.note_id
