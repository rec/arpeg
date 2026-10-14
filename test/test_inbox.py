from concurrent.futures import ThreadPoolExecutor
from queue import Empty

import pytest

from arpeg.inbox import Inbox, InputFailure, InputOverflow


def test_draining_restores_byte_and_packet_capacity_in_fifo_order() -> None:
    inbox = Inbox[str](packets=2, bytes=5, max_message_bytes=3, failure=InputFailure())
    inbox.put("first", 3)
    inbox.put("second", 2)
    assert inbox.get_nowait() == "first"
    inbox.put("third", 3)
    assert inbox.get_nowait() == "second"
    assert inbox.get_nowait() == "third"
    with pytest.raises(Empty):
        inbox.get_nowait()


@pytest.mark.parametrize(
    "packets, budget, maximum, sizes",
    [
        (1, 10, 10, [1, 1]),
        (10, 3, 10, [2, 2]),
        (10, 10, 3, [4]),
    ],
)
def test_each_capacity_limit_ends_input_and_latches_failure(
    packets: int, budget: int, maximum: int, sizes: list[int]
) -> None:
    failure = InputFailure()
    inbox = Inbox[int](
        packets=packets, bytes=budget, max_message_bytes=maximum, failure=failure
    )
    for s in sizes:
        inbox.put(s, s)
    with pytest.raises(InputOverflow, match="MIDI input overflow"):
        inbox.get_nowait()
    inbox.put(1, 1)
    with pytest.raises(InputOverflow):
        failure.check()


def test_concurrent_producers_share_capacity_and_notify_another_queue() -> None:
    failure = InputFailure()
    midi = Inbox[int](packets=128, bytes=128, max_message_bytes=1, failure=failure)
    commands = Inbox[str](packets=1, bytes=4, max_message_bytes=4, failure=failure)
    with ThreadPoolExecutor(max_workers=2) as pool:
        list(pool.map(lambda n: midi.put(n, 1), range(129)))
    with pytest.raises(InputOverflow):
        commands.get_nowait()
