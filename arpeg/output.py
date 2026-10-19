"""Optional MIDI byte budget and ownership of successfully sent notes."""

from collections.abc import Callable

from meedee import codecs
from meedee.messages import (
    Message,
    NoteOff,
    NoteOn,
)
from pydantic import BaseModel, Field


class OutputOverload(RuntimeError):
    """The destination's configured bandwidth has been exceeded."""


class MidiDelivery(BaseModel):
    bytes_per_second: int | None = Field(default=None, gt=0, frozen=True)
    burst_bytes: int = Field(default=64, gt=0, frozen=True)
    credit: int = 0
    updated_ns: int | None = None
    failed: bool = False
    owned: dict[int, int] = Field(default_factory=dict)

    def send(
        self,
        messages: list[Message],
        at_ns: int,
        write: Callable[[Message], None],
    ) -> None:
        if self.failed:
            raise OutputOverload("MIDI output bandwidth exceeded; play session ended")
        if self.bytes_per_second is not None and messages:
            ceiling = self.burst_bytes * 1_000_000_000
            available = (
                ceiling
                if self.updated_ns is None
                else min(
                    ceiling,
                    self.credit + (at_ns - self.updated_ns) * self.bytes_per_second,
                )
            )
            required = sum(len(codecs.encode(m)) for m in messages) * 1_000_000_000
            if required > available:
                self.failed = True
                raise OutputOverload(
                    "MIDI output bandwidth exceeded; play session ended"
                )
            self.credit = available - required
            self.updated_ns = at_ns
        for message in messages:
            write(message)
            self._record(message)

    def cleanup(
        self, messages: list[Message], write: Callable[[Message], None]
    ) -> None:
        """Preserve supplied release velocities, then release any remaining owners."""
        for message in messages:
            if not (
                isinstance(message, NoteOff)
                or isinstance(message, NoteOn)
                and message.velocity == 0
            ):
                continue
            note = (message.channel - 1) * 128 + message.note
            if note in self.owned:
                write(message)
                self._record(message)
        while self.owned:
            note = min(self.owned)
            message = NoteOff(channel=note // 128 + 1, note=note % 128, velocity=0)
            write(message)
            self._record(message)

    def _record(self, message: Message) -> None:
        if not isinstance(message, (NoteOn, NoteOff)):
            return
        note = (message.channel - 1) * 128 + message.note
        if isinstance(message, NoteOn) and message.velocity > 0:
            self.owned[note] = self.owned.get(note, 0) + 1
        else:
            if (count := self.owned.get(note, 0)) > 1:
                self.owned[note] = count - 1
            else:
                self.owned.pop(note, None)
