import pytest
from meedee.messages import ControlChange, Message, NoteOff, NoteOn, TimingClock

from arpeg.output import MidiDelivery, OutputOverload


def test_disabled_budget_preserves_dense_controller_samples() -> None:
    delivery = MidiDelivery(burst_bytes=1)
    messages = [
        ControlChange(channel=1, controller=2, value=n % 128) for n in range(256)
    ]
    sent: list[Message] = []
    delivery.send(messages, 0, sent.append)
    assert sent == messages


@pytest.mark.parametrize(
    "at_ns, accepted", [(999_999_999, False), (1_000_000_000, True)]
)
def test_byte_credit_refills_exactly_with_elapsed_time(
    at_ns: int, accepted: bool
) -> None:
    delivery = MidiDelivery(bytes_per_second=3, burst_bytes=3)
    message = ControlChange(channel=1, controller=2, value=64)
    sent: list[Message] = []
    delivery.send([message], 0, sent.append)
    if accepted:
        delivery.send([message], at_ns, sent.append)
        assert sent == [message, message]
    else:
        with pytest.raises(OutputOverload):
            delivery.send([message], at_ns, sent.append)
        assert sent == [message]


def test_long_idle_does_not_accumulate_more_than_the_burst_allowance() -> None:
    delivery = MidiDelivery(bytes_per_second=3, burst_bytes=3)
    message = TimingClock()
    sent: list[Message] = []
    delivery.send([message], 0, sent.append)
    with pytest.raises(OutputOverload):
        delivery.send([message] * 4, 100_000_000_000, sent.append)
    assert sent == [message]
    with pytest.raises(OutputOverload):
        delivery.send([message], 200_000_000_000, sent.append)


def test_refused_handoff_sends_no_entry_controls_and_releases_the_old_note() -> None:
    delivery = MidiDelivery(bytes_per_second=3, burst_bytes=6)
    old = NoteOn(channel=1, note=60, velocity=100)
    release = NoteOff(channel=1, note=60, velocity=0)
    breath = ControlChange(channel=1, controller=2, value=80)
    new = NoteOn(channel=1, note=64, velocity=90)
    sent: list[Message] = []
    delivery.send([old], 0, sent.append)
    with pytest.raises(OutputOverload):
        delivery.send([release, breath, new], 0, sent.append)
    # The player now owns the new note, but only the old note was sent.
    delivery.cleanup([NoteOff(channel=1, note=64, velocity=64)], sent.append)
    assert sent == [old, release]


def test_cleanup_preserves_release_velocity_and_counts_velocity_zero_releases() -> None:
    delivery = MidiDelivery()
    attack = NoteOn(channel=3, note=60, velocity=100)
    zero = NoteOn(channel=3, note=60, velocity=0)
    release = NoteOff(channel=3, note=60, velocity=20)
    sent: list[Message] = []
    delivery.send([attack, attack, zero], 0, sent.append)
    delivery.cleanup([release], sent.append)
    delivery.cleanup([], sent.append)
    assert sent == [attack, attack, zero, release]


def test_failed_send_is_not_recorded_as_an_owned_note() -> None:
    delivery = MidiDelivery()
    sent: list[Message] = []
    old = NoteOn(channel=1, note=60, velocity=64)
    delivery.send([old], 0, sent.append)

    def disconnected(message: Message) -> None:
        raise OSError("disconnected")

    with pytest.raises(OSError):
        delivery.send([NoteOn(channel=1, note=64, velocity=64)], 0, disconnected)
    delivery.cleanup([], sent.append)
    assert sent == [old, NoteOff(channel=1, note=60, velocity=0)]
