# arpeg standalone MIDI player

This archive contains a native executable and three example profiles. It needs
no Python, Rust compiler, enge, sample assets, or audio engine. It uses your
operating system's MIDI service and the normal driver for your MIDI interface.
MIDI transport uses meedee with statically linked libremidi; no separate
libremidi installation is required. The Rust executable does not use Python's
optional process isolation, so an unresponsive native driver can block it.

Download the archive for your platform from the GitHub release:

| Platform | Target in filename |
| --- | --- |
| Apple Silicon Mac | aarch64-apple-darwin |
| Intel Mac | x86_64-apple-darwin |
| Windows x64 | x86_64-pc-windows-msvc |
| Linux x64 | x86_64-unknown-linux-gnu |

Mac builds run on macOS 15 or later. Linux builds target Ubuntu 24.04; other
distributions need compatible glibc and ALSA runtime libraries. Windows uses
the operating system MIDI APIs. These archives are not signed or notarized.
Each ZIP has a companion `.sha256` file with its SHA-256 checksum.

Extract the archive and open a terminal in its directory. On Mac/Linux:

```sh
./arpeg validate profiles/up.toml
./arpeg list-ports
./arpeg play profiles/up.toml 0 1 120
```

On Windows PowerShell, use `./arpeg.exe` instead of `./arpeg`. Replace `0` and
`1` with the source and destination indices from `list-ports`. The last number
is BPM. Input MIDI uses channel 1; output uses each preset lane's configured
channel, defaulting to 1. Configure the synth's receiving channels accordingly.

`up.toml` plays held keys in ascending order. `live-latch.toml` keeps the chord
after key release. `history-wind.toml` replays completed notes with recorded
breath and bend; the destination synth must support those controllers.

During playing, type `pause`, `continue`, `tempo 90`, or `quit` and press Enter.
The full command list appears when playback starts. External MIDI Clock is
available with `--clock external`; `--clock-source INDEX` selects a separate
clock input. Stop and clock loss release the output notes the player owns.
Hardware timing and receiver articulation still need physical verification.

Input defaults to 4096 queued packets, 1048576 queued payload bytes, and 65536
bytes per packet or assembled SysEx. Change these with `--input-packets`,
`--input-bytes`, and `--max-message-bytes`; all require positive integers.
Overflow releases owned notes, reports the error, and ends playback. Restart
the session to obtain fresh input state. There is no queue-age cutoff. Console
input is also bounded at 32 queued commands and 1024 bytes per line.
These limits cover arpeg's retained input; meedee separately bounds native
receive payloads to the configured input-byte budget per input client. Driver
buffers and allocation/object overhead are outside those limits. Observed native
receive drops or callback panics also end the session rather than silently
continuing with incomplete note/clock state. Native sends and closes have no
process-isolation deadline. Cleanup failures are reported; a successful send
means backend acceptance, not confirmed instrument delivery.

Output bandwidth protection is disabled by default. Enable it with
`--output-bytes-per-second RATE`; `--output-burst-bytes BYTES` sets the burst
allowance (default 64). Both values require positive integers. A DIN destination
can use `--output-bytes-per-second 3125`. The host admits each complete output
batch before sending and adds no pacing queue or controller thinning. A batch
larger than the allowance is refused even after idle time. On overload, playback
ends with an error; cleanup bypasses the budget and sends Note Offs only for notes
accepted by the port. Driver buffers and actual device delivery are not measured.

## MIDI transport notices

The executable statically includes meedee and libremidi. Their notices follow.

### meedee

MIT License

Copyright (c) 2026 meedee contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

### libremidi and its derived code

This software is derived from two other libraries whose license is reproduced thereafter:

RtMidi (https://github.com/thestk/rtmidi) for real-time input / output, under the MIT license.
ModernMidi (https://github.com/ddiakopoulos/modern-midi) for MIDI file (SMF) input / output, under the BSD 2-clause license.

In addition, new contributions to this software are under the following license (BSD 2-clause):

~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Copyright (c) 2017-2023, Jean-Michaël Celerier & the libremidi contributors, all rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this
  list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright notice,
  this list of conditions and the following disclaimer in the documentation
  and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

RtMidi: realtime MIDI i/o C++ classes
Copyright (c) 2003-2017 Gary P. Scavone

Permission is hereby granted, free of charge, to any person
obtaining a copy of this software and associated documentation files
(the "Software"), to deal in the Software without restriction,
including without limitation the rights to use, copy, modify, merge,
publish, distribute, sublicense, and/or sell copies of the Software,
and to permit persons to whom the Software is furnished to do so,
subject to the following conditions:

The above copyright notice and this permission notice shall be
included in all copies or substantial portions of the Software.

Any person wishing to distribute modifications to the Software is asked to send the modifications to the original developer so that they can be incorporated into the canonical version.  This is,
however, not a binding provision of this license.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR
ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~

Copyright (c) 2015, Dimitri Diakopoulos All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this
  list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright notice,
  this list of conditions and the following disclaimer in the documentation
  and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
