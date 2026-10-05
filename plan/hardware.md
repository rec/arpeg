# arpeg as a small MIDI appliance

## Goal and scope

Build a small plastic box containing a Raspberry Pi, one TRS MIDI input, and
one TRS MIDI output. It boots into arpeg without a screen, keyboard, or computer
connection. This document plans the work; it does not implement a Linux backend
or constitute a reviewed electrical schematic.

Assumption: TRS means **3.5 mm TRS MIDI**, carrying MIDI 1.0 messages, rather than
audio. The box also needs a separate power connection. The proposed starting
point is a Pi Zero 2 W, Type A MIDI sockets, external 5 V power, and a fixed
profile with internal tempo. These choices remain proposals until the parts
and intended instruments are confirmed.

Additional work beyond the prompt: None.

## 1. What has to fit inside the box?

There are four jobs:

| Part | Job |
| --- | --- |
| Raspberry Pi | Run arpeg and schedule MIDI events |
| MIDI interface board | Convert the external MIDI electrical signal to and from the Pi's serial pins |
| Power and storage | Supply stable power and boot the operating system |
| Enclosure and mounts | Hold the boards and sockets securely |

Conceptually, the signal path is:

```text
TRS MIDI IN -> isolated MIDI receiver -> Pi UART RX
                                        arpeg
TRS MIDI OUT <- MIDI output driver    <- Pi UART TX

External 5 V supply -> Pi and interface board
```

A UART is the Pi's hardware serial transmitter and receiver. The interface
board handles the electrical conversion; software handles MIDI messages.

The Pi Zero 2 W has a quad-core 1 GHz processor, 512 MB RAM, a microSD slot,
and a 65 x 30 mm board. It is a reasonable starting point for this workload.
CPU capacity is unlikely to be the difficult part, but that is an engineering
expectation, not a measured arpeg result. Timing under Linux and output bandwidth
still need measurement. Use the Zero 2 W rather than assuming the original
Zero's processor and software targets are interchangeable.
[Raspberry Pi product brief](https://datasheets.raspberrypi.com/rpizero2/raspberry-pi-zero-2-w-product-brief.pdf)

## 2. The MIDI interface is a small, separate circuit

### Input

MIDI input needs an optically isolated receiver. An optocoupler transfers the
signal through light, keeping the external instrument's electrical connection
separate from the Pi's logic ground. Its output must provide a signal suitable
for the Pi's 3.3 V UART input.

### Output

MIDI output needs a driver and the appropriate current-limiting components.
MIDI uses a current loop; a TRS socket is not a substitute for that circuit.
Use the MIDI Association's reference circuits, choosing component values for
the actual supply and driver rather than copying an arbitrary 5 V tutorial.
[MIDI electrical specification](https://www.midi.org/wp-content/uploads/wpforo/default_attachments/1709416667-ca33-MIDI-10-Electrical-Specification-Update.pdf)

All Pi UART pins use 3.3 V logic and must not receive 5 V signals. For the
finished design, have someone familiar with MIDI electronics review the
receiver, output driver, socket wiring, and power connections before assembly.
[Raspberry Pi UART documentation](https://www.raspberrypi.com/documentation/computers/configuration.html#configure-uarts)

### TRS wiring

Use **Type A** for the proposed design and label both sockets `MIDI IN` and
`MIDI OUT`, with `TRS A` visible on the enclosure. Type A and Type B interchange
tip and ring, so matching connector size alone does not establish compatibility.
Check the controller and synth manuals and obtain the corresponding adapters.
The MIDI Association defines the standardized TRS connection.
[TRS MIDI specification](https://midi.org/specification-for-trs-adapters-adopted-and-released),
[connector wiring background](https://midi.org/updated-how-to-make-your-own-3-5mm-mini-stereo-trs-to-midi-5-pin-din-cables)

### Recommended route for someone new to hardware

First look for an **assembled Pi-compatible MIDI input/output board** with a
published schematic, a 3.3 V receiver output, and documented UART support.
Check its actual dimensions, connector arrangement, and Pi Zero compatibility
before ordering. A board with DIN sockets may be useful for testing but may
not fit the final TRS enclosure.

If a suitable assembled board cannot be found, commission a small interface
board from an electronics designer. Ask for one isolated input, one driven
output, two Type A TRS sockets, and a connection to the Zero's UART and power.
This is a bounded electronics job; the designer does not need to implement
the arpeggiator.

The first unit can use a professionally soldered prototype board. For several
units, order assembled printed circuit boards. Avoid relying on loose
breadboard connections inside a device that will travel.

## 3. Software work needed before it can play on the Pi

The current Rust MIDI executable uses **CoreMIDI on macOS** for live playing.
Its `play` and `list-ports` commands are compiled only on macOS. Copying the
current executable to a Pi will not provide live MIDI.

Keep the Python implementation as the reference and reuse `arpeg-core` for the
appliance. Add a Linux host that connects the hardware UART to the existing
event decisions. No audio renderer is required for this MIDI device.

The host needs to:

1. Open the UART at MIDI's **31,250 baud, 8 data bits, no parity, one stop bit**.
   Configure raw bytes with no terminal echo, newline conversion, or flow
   control. Confirm the actual baud rate on the selected Pi setup.
2. Decode MIDI byte streams, including running status, velocity-zero releases,
   and realtime bytes interleaved with other messages. Serial read boundaries
   are not MIDI message boundaries.
3. Timestamp input against a monotonic clock and feed existing live engines.
4. Schedule output from the next engine deadline and handle partial serial
   writes without losing or duplicating bytes.
5. Make controller, channel, system-message, and clock handling explicit.
   Do not blindly forward input notes alongside generated notes.
6. Release owned notes on orderly stop. A power cut cannot send those releases,
   so test the receiving synth's recovery behavior too.

Use the PL011 UART for predictable baud timing. On the Zero 2 W, Bluetooth
normally occupies that UART; configure it for the GPIO connector, typically by
disabling Bluetooth, and disable the serial login console. The mini UART's
baud timing depends on the core clock. Verify the resulting device mapping;
do not assume `/dev/serial0` always names the same hardware.
[Raspberry Pi UART configuration](https://www.raspberrypi.com/documentation/computers/configuration.html#configure-uarts)

MIDI is low bandwidth, but that also limits what can be emitted. At 31,250 baud
with ten wire bits per byte, a byte occupies 320 microseconds and a three-byte
note message occupies 960 microseconds without running status. Dense repeats,
controller replay, and simultaneous notes can fill the output link. Measure
both scheduling delay and serial transmission delay.
[MIDI 1.0 specification](https://midi.org/midi-1-0-detailed-specification)

## 4. Make it behave like an appliance

Start with Raspberry Pi OS Lite and a release build of the Rust executable.
Configure arpeg as a boot service so no login or terminal session is required.
Give it only the permissions needed for its serial device and configuration.

For the first prototype:

- Load one profile and BPM from a file.
- Use Wi-Fi/SSH during development to install builds and inspect logs.
- Keep playback independent of Wi-Fi availability.
- Provide a visible indication when arpeg is actually ready, rather than merely
  when the Pi has power. A small LED is a proposed addition to the two MIDI ports.
- Record startup errors and make a failed startup distinguishable from a silent
  but healthy arpeggiator.

Once stable, configure a read-only root filesystem with a RAM overlay and
minimize persistent writes. This reduces filesystem damage from unplugging the
box; it is not a guarantee against every storage or power failure. Define how
profiles and firmware are updated, because writes to a RAM overlay disappear
on reboot. Keep a restorable SD-card image.
[Raspberry Pi filesystem resilience guide](https://pip.raspberrypi.com/categories/685-whitepapers-app-notes-compliance-guides/documents/RP-003610-WP/Making-a-more-resilient-file-system.pdf)

External MIDI clock is a separate software milestone. The current host uses
an internal BPM clock. If this box must follow a drum machine, complete clock
acquisition and Start/Continue/Stop behavior before declaring it ready.

## 5. Power, sockets, and enclosure

Use an external regulated 5 V supply, initially through the Pi's existing
micro-USB power socket. The Zero 2 W product brief specifies a 5 V, 2.5 A supply.
Do not assume the MIDI cable can power the box.
[Power requirements](https://datasheets.raspberrypi.com/rpizero2/raspberry-pi-zero-2-w-product-brief.pdf)

Buy or print a plastic project enclosure after measuring the assembled
prototype, including headers, cable plugs, socket bodies, and mounting clearance.
A Pi-only case may not have room for the interface board.

The enclosure should provide:

- Two accessible, clearly labelled TRS sockets.
- Access to the power socket without putting sideways force on it.
- Standoffs and screws securing both boards. Cable insertion forces must be
  carried by the enclosure or supported PCB, not by loose wires.
- Clearance below solder joints and between boards.
- Access to the SD card, or a removable lid for servicing.
- Enough thermal clearance for reliable operation, checked with the lid closed.

For a first box, use a stock project enclosure with drilled openings, or a
3D-printed enclosure from a CAD model. A local makerspace or fabrication service
can help with drilling, printing, and soldering. Injection moulding is unnecessary
for one or a few units.

## 6. Build in stages

| Stage | Work | Evidence before proceeding |
| --- | --- | --- |
| Bench computer | Boot the Pi, install a release build, load a profile | Runs without a desktop or network dependency |
| Electrical interface | Attach a reviewed MIDI board and test UART input/output | Correct bytes in both directions with the intended controller and synth |
| Live arpeggiator | Connect UART MIDI to the existing core | Shared fixtures pass; actual notes, releases, and expression work |
| Appliance behavior | Add boot service, readiness indication, and storage policy | Starts unattended; configuration survives the intended update procedure |
| First enclosure | Mount the proven assembly in a plastic box | Plugs fit; boards stay fixed; closed-box operation is reliable |
| Repeatable build | Finalize wiring, drawings, assembly, and SD image | A second unit can be assembled without inventing new steps |

Test long held notes, overlapping notes, latch changes, dense repeated hits,
breath and pitch bend, cable disconnect/reconnect, service stop/restart, and
repeated power cycles. Measure boot-to-ready time and timing under realistic
message load. If using external clock, also test tempo changes, transport
commands, and clock loss. Record results separately from desktop unit tests.

## 7. Parts and help to obtain

Initial parts list, subject to the selected interface board:

- Pi Zero 2 W, preferably with its GPIO header already soldered.
- microSD card, suitable 5 V supply, and power cable.
- Assembled MIDI interface board or a commissioned soldered prototype.
- Two 3.5 mm Type A TRS MIDI sockets if not already on that board.
- MIDI cables/adapters matching the actual instruments.
- Plastic enclosure, standoffs, screws, and any internal wiring connectors.
- Proposed readiness LED and its supporting circuit.

Borrow or obtain a multimeter. Have the electronics helper check supply voltages
before connecting GPIO. A logic analyser or oscilloscope is useful for verifying
baud timing and measuring latency; the helper may already own one.

If commissioning the hardware, request a schematic, bill of materials, PCB
manufacturing files, assembly instructions, enclosure drawing, and a simple
test procedure. Obtain editable source files as well as exported manufacturing
files. This lets another person repair or reproduce the device later.

## 8. Decisions before buying or commissioning

1. **Exact Pi model:** proposed Zero 2 W; confirm whether a Pi is already owned.
2. **Connector compatibility:** proposed 3.5 mm Type A; check both instruments.
3. **Tempo:** fixed internal BPM first, or external clock required immediately?
4. **Controls:** one fixed profile, remote configuration, or physical controls?
   Knobs/buttons change both software and enclosure requirements.
5. **Power:** proposed external micro-USB supply. Batteries or another power
   connector require additional circuitry and space.
6. **Quantity:** one personal prototype or a reproducible small batch?
7. **Build assistance:** assembled board, local soldering help, or commissioned PCB?

The next concrete step is to identify the instruments and select or commission
the MIDI interface. Then prove the Pi and interface on the bench before designing
the final box.
