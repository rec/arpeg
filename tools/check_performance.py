"""Measure the live player with synthetic MIDI, without opening device ports."""

from fractions import Fraction
from json import dumps
from pathlib import Path
from time import perf_counter_ns, sleep

import tyro
from meedee import codecs
from meedee.messages import (
    ControlChange,
    NoteOn,
)
from pydantic import BaseModel, Field

from arpeg.midi import MidiPlayer
from arpeg.profile import parse_profile


class CheckPerformance(BaseModel, frozen=True):
    duration_ms: int = Field(default=2000, ge=100)
    controller_hz: int = Field(default=1000, ge=1, le=100_000)
    stall_ms: int = Field(default=0, ge=0)

    def run(self) -> None:
        root = Path(__file__).resolve().parents[1]
        player = MidiPlayer(
            profile=parse_profile((root / "conformance/live-wind.toml").read_text())
        )
        duration = self.duration_ms * 1_000_000
        interval = 1_000_000_000 // self.controller_hz
        next_input = interval
        samples: list[int] = []
        lateness: list[int] = []
        owned: dict[int, int] = {}
        attacks = 0
        releases = 0
        wire_bytes = 0
        exhausted = False
        stalled = False
        origin = perf_counter_ns()
        player.accept(0, NoteOn(channel=1, note=60, velocity=100))
        while (now := perf_counter_ns() - origin) < duration:
            if not stalled and self.stall_ms and now >= duration // 2:
                sleep(self.stall_ms / 1000)
                stalled = True
                continue
            started = perf_counter_ns()
            while next_input <= now:
                messages = player.accept(
                    next_input, ControlChange(channel=1, controller=2, value=64)
                )
                wire_bytes += sum(len(list(codecs.encode(m))) for m in messages)
                next_input += interval
            messages = player.advance(now)
            wire_bytes += sum(len(list(codecs.encode(m))) for m in messages)
            batch = player.take_events()
            exhausted |= batch.exhausted
            delivered = perf_counter_ns() - origin
            for event in batch.notes:
                due = Fraction(event.at) * 500_000_000
                lateness.append(max(0, delivered - int(due)))
                if event.port == "note_start":
                    owned[event.occurrence] = event.key
                    attacks += 1
                else:
                    if owned.pop(event.occurrence, None) != event.key:
                        raise RuntimeError("Release does not match an owned occurrence")
                    releases += 1
            samples.append(perf_counter_ns() - started)
            sleep(0.001)
        wire_bytes += sum(
            len(list(codecs.encode(m))) for m in player.stop(perf_counter_ns() - origin)
        )
        for event in player.take_events().notes:
            if (
                event.port != "note_end"
                or owned.pop(event.occurrence, None) != event.key
            ):
                raise RuntimeError("Stop did not release an owned occurrence")
            releases += 1
        if owned or attacks != releases or exhausted:
            raise RuntimeError(
                "Load check left owned notes or exhausted an event buffer"
            )
        elapsed = perf_counter_ns() - origin
        samples.sort()
        lateness.sort()
        print(
            dumps(
                {
                    "runtime": "python",
                    "controller_hz": self.controller_hz,
                    "stall_ms": self.stall_ms,
                    "elapsed_ms": elapsed / 1_000_000,
                    "polls": len(samples),
                    "batch_p99_us": samples[(len(samples) * 99 + 99) // 100 - 1] / 1000,
                    "batch_max_us": samples[-1] / 1000,
                    "note_lateness_p99_us": (
                        lateness[(len(lateness) * 99 + 99) // 100 - 1] / 1000
                    ),
                    "note_lateness_max_us": lateness[-1] / 1000,
                    "wire_bytes_per_second": wire_bytes * 1_000_000_000 / elapsed,
                    "attacks": attacks,
                    "releases": releases,
                    "owned_after_stop": len(owned),
                },
                sort_keys=True,
            )
        )


if __name__ == "__main__":
    tyro.cli(CheckPerformance).run()
