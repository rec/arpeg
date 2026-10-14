# arpeg standalone MIDI player

This archive contains a native executable and three example profiles. It needs
no Python, Rust compiler, enge, sample assets, or audio engine. It uses your
operating system's MIDI service and the normal driver for your MIDI interface.

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
is BPM. Source/destination MIDI uses channel 1; configure the synth accordingly.

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
These limits cover arpeg's retained input, not MIDI-library/driver buffers.
