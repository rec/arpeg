"""Capacity-limited callback queues with a shared, latched failure."""

from queue import Queue
from threading import Lock
from typing import Generic, TypeVar

from pydantic import BaseModel, ConfigDict, Field

T = TypeVar("T")


class InputOverflow(RuntimeError):
    """Input can no longer be reproduced faithfully; end the session."""


class InputFailure(BaseModel):
    reason: str | None = None
    lock: Lock = Field(default_factory=Lock)

    def check(self) -> None:
        if self.reason is not None:
            raise InputOverflow(self.reason)

    model_config = ConfigDict(arbitrary_types_allowed=True)


class Inbox(BaseModel, Generic[T]):
    """Reserve count and payload capacity before retaining callback input."""

    packets: int = Field(gt=0)
    bytes: int = Field(gt=0)
    max_message_bytes: int = Field(gt=0)
    failure: InputFailure
    label: str = "MIDI input"
    items: Queue[tuple[T, int]] = Field(default_factory=Queue)
    queued_bytes: int = 0

    def put(self, value: T, size: int) -> None:
        with self.failure.lock:
            if self.failure.reason is not None:
                return
            if (
                size > self.max_message_bytes
                or self.items.qsize() >= self.packets
                or self.queued_bytes + size > self.bytes
            ):
                self.failure.reason = f"{self.label} overflow; play session ended"
                return
            self.items.put_nowait((value, size))
            self.queued_bytes += size

    def get_nowait(self) -> T:
        with self.failure.lock:
            self.failure.check()
            value, size = self.items.get_nowait()
            self.queued_bytes -= size
            return value

    model_config = ConfigDict(arbitrary_types_allowed=True)
