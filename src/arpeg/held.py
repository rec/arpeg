"""Deterministic held-note arpeggiation on an exact local beat grid."""

from fractions import Fraction
from itertools import groupby
from math import ceil

from ufor.arpeggiator import (
    ArpeggiatorScore,
    Ascending,
    Descending,
    Grid,
    HeldBank,
    LatchedBank,
    Played,
)
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
    """Render supported held/grid selections before ``through``."""
    body = profile.body
    if not isinstance(body.bank, (HeldBank, LatchedBank)):
        raise ValueError("held rendering requires a held or latched bank")
    selection = body.selection
    if not isinstance(selection, (Ascending, Descending, Played)):
        raise ValueError("held rendering requires a classic note selection")
    if isinstance(selection, (Ascending, Descending)) and selection.key != "pitch":
        raise ValueError("held rendering requires pitch selection")
    if isinstance(selection, Ascending) and selection.repeats != 1:
        raise ValueError("held rendering does not support repeated selections")
    if not isinstance(body.rhythm, Grid):
        raise ValueError("held rendering requires grid rhythm")
    if body.retrigger != "on_empty":
        raise ValueError("held rendering does not support bank-edit retrigger")
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
    previous_key: tuple[Fraction, str] | None = None
    revision = 0
    for index in range(ceil(through / step)):
        at = index * step
        active = _bank_at(note_times, body.bank, at)
        bank = {n.note_id for n in active}
        if bank != previous_bank:
            revision += 1
            previous_bank = bank
        if not active:
            previous_key = None
            continue
        ordered = sorted(active, key=lambda n: _selection_key(n, selection))
        note = next(
            (
                n
                for n in ordered
                if previous_key is None or _selection_key(n, selection) > previous_key
            ),
            ordered[0],
        )
        previous_key = _selection_key(note, selection)
        gate_end = at + step * body.gate
        for boundary in sorted(
            {
                t
                for _, start, end in note_times
                for t in (start, end)
                if at < t < gate_end
            }
        ):
            if not _bank_at(note_times, body.bank, boundary):
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


def _selection_key(
    note: SourceNote, selection: Ascending | Descending | Played
) -> tuple[Fraction, str]:
    if isinstance(selection, Played):
        position = Fraction(note.onset_tick)
        return (
            -position if selection.direction == "reverse" else position
        ), note.note_id
    assert note.key is not None
    pitch = Fraction(note.key)
    return (-pitch if isinstance(selection, Descending) else pitch), note.note_id


def _bank_at(
    note_times: list[tuple[SourceNote, Fraction, Fraction]],
    bank: HeldBank | LatchedBank,
    at: Fraction,
) -> list[SourceNote]:
    if isinstance(bank, HeldBank):
        return [n for n, start, end in note_times if start <= at < end]
    active: list[SourceNote] = []
    entries = sorted(
        ((start, n) for n, start, _ in note_times if start <= at), key=lambda p: p[0]
    )
    for _, group in groupby(entries, key=lambda p: p[0]):
        notes = [n for _, n in group]
        if bank.update == "replace":
            active = notes
        elif bank.update == "add":
            active.extend(notes)
        else:
            for key, same_pitch in groupby(
                sorted(notes, key=lambda n: n.key), key=lambda n: n.key
            ):
                if any(n.key == key for n in active):
                    active = [n for n in active if n.key != key]
                else:
                    active.extend(same_pitch)
    return active
