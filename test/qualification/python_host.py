"""Explicit virtual-only macOS qualification, excluded from ordinary collection."""

from collections.abc import Iterator
from contextlib import ExitStack, contextmanager
from os import environ, killpg
from pathlib import Path
from queue import Empty, Queue
from signal import SIGKILL
from subprocess import PIPE, Popen, TimeoutExpired
from sys import executable, platform
from threading import Thread
from time import monotonic
from uuid import uuid4

import pytest
from meedee import codecs, events, midi
from meedee.messages import Message, NoteOff, NoteOn


@pytest.mark.skipif(platform != "darwin", reason="CoreMIDI virtual-only qualification")
@pytest.mark.parametrize("isolated", [False, True], ids=["in_process", "isolated"])
def test_python_host(isolated: bool, request: pytest.FixtureRequest) -> None:
    if environ.get("ARPEG_PYTHON_HOST_CHILD") == "1":
        exercise_host(isolated)
        return
    root = Path(__file__).resolve().parents[2]
    child = Popen(
        [executable, "-m", "pytest", "-q", "-s", request.node.nodeid],
        cwd=root,
        env={**environ, "ARPEG_PYTHON_HOST_CHILD": "1"},
        stdout=PIPE,
        stderr=PIPE,
        text=True,
        start_new_session=True,
    )
    try:
        stdout, stderr = child.communicate(timeout=45)
    except TimeoutExpired:
        killpg(child.pid, SIGKILL)
        stdout, stderr = child.communicate(timeout=5)
        pytest.fail(f"native flow exceeded 45 seconds\n{stdout}\n{stderr}")
    print(stdout, end="")
    assert child.returncode == 0, stderr + stdout


def exercise_host(isolated: bool) -> None:
    root = Path(__file__).resolve().parents[2]
    prefix = f"arpeg-python-{uuid4().hex}"
    received: Queue[bytes] = Queue(maxsize=128)

    def receive(packet: events.ReceivedPacket) -> None:
        assert isinstance(packet.packet, bytes)
        received.put_nowait(packet.packet)

    with ExitStack() as stack:
        source = stack.enter_context(
            midi.Output(isolated=False).create_virtual(prefix + "-notes")
        )
        clock = stack.enter_context(
            midi.Output(isolated=False).create_virtual(prefix + "-clock")
        )
        sink = stack.enter_context(
            midi.Input(isolated=False).create_virtual(prefix + "-sink", receive)
        )
        command = [str(Path(executable).with_name("arpeg-python"))]
        listing = Popen(
            command + ["list-ports", "--no-isolated"],
            cwd=root,
            stdout=PIPE,
            stderr=PIPE,
            text=True,
        )
        stdout, stderr = listing.communicate(timeout=5)
        assert listing.returncode == 0, stderr
        source_index = port_index(stdout, "Sources:", prefix + "-notes")
        clock_index = port_index(stdout, "Sources:", prefix + "-clock")
        destination_index = port_index(stdout, "Destinations:", prefix + "-sink")
        base = command + [
            "play",
            "--profile",
            "conformance/up.toml",
            "--source",
            str(source_index),
            "--destination",
            str(destination_index),
        ]
        if not isolated:
            base.append("--no-isolated")
        for selected_clock in [None, source_index, clock_index]:
            while not received.empty():
                received.get_nowait()
            args = base.copy()
            if selected_clock is not None:
                args += ["--clock", "external", "--clock-source", str(selected_clock)]
            with host_process(args, root) as host:
                source.send(b"\x90\x3c\x64")
                timing = None
                if selected_clock is not None:
                    timing = source if selected_clock == source_index else clock
                    timing.send(b"\xfa")
                await_note(received, NoteOn(channel=1, note=60, velocity=100), timing)
                assert host.stdin is not None
                host.stdin.write("quit\n")
                host.stdin.flush()
                host.wait(timeout=5)
                stdout, stderr = host.communicate(timeout=5)
                assert host.returncode == 0, stderr + stdout
                await_note(received, NoteOff(channel=1, note=60, velocity=0))
            print(
                f"Python host delivery/quit passed: "
                f"isolated={isolated}, clock={selected_clock}",
                flush=True,
            )
        with host_process(base + ["--input-bytes", "2"], root) as host:
            source.send(b"\x90\x3c\x64")
            host.wait(timeout=5)
            stdout, stderr = host.communicate(timeout=5)
            assert host.returncode != 0
            assert "MIDI transport input lost" in stderr, stderr + stdout
        assert received.empty(), "refused input produced output"
        print(
            f"Python host transport-loss failure passed: isolated={isolated}",
            flush=True,
        )
        assert sink.dropped_packets == 0
        assert sink.worker_failure is None


@contextmanager
def host_process(args: list[str], root: Path) -> Iterator[Popen[str]]:
    child = Popen(
        args,
        cwd=root,
        env={**environ, "PYTHONUNBUFFERED": "1"},
        stdin=PIPE,
        stdout=PIPE,
        stderr=PIPE,
        text=True,
    )
    try:
        assert child.stdout is not None
        ready: Queue[str] = Queue(maxsize=1)
        stdout = child.stdout
        Thread(target=lambda: ready.put_nowait(stdout.readline()), daemon=True).start()
        try:
            line = ready.get(timeout=10)
        except Empty:
            pytest.fail("Python host did not become ready within ten seconds")
        assert line.startswith("Playing with"), line
        yield child
    finally:
        if child.poll() is None:
            child.kill()
        child.communicate(timeout=5)


def port_index(listing: str, section: str, name: str) -> int:
    for line in listing.split(section, 1)[1].splitlines():
        if ": " in line:
            index, found = line.strip().split(": ", 1)
            if found == name:
                return int(index)
    raise AssertionError(f"test-owned endpoint missing: {name}")


def await_note(
    received: Queue[bytes],
    expected: Message,
    clock: midi.OutputConnection | None = None,
) -> None:
    deadline = monotonic() + 3
    decoder = codecs.Decoder()
    while monotonic() < deadline:
        if clock is not None:
            clock.send(b"\xf8")
        try:
            packet = received.get(timeout=min(0.02, max(0, deadline - monotonic())))
        except Empty:
            continue
        for message in decoder.feed(packet):
            assert not isinstance(message, codecs.Diagnostic), message
            if message == expected:
                return
    raise AssertionError(f"missing output: {expected}")
