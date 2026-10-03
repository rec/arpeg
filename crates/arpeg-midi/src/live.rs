//! A small CoreMIDI host for the held-note preset.

use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use arpeg_core::{
    Bank, Beat,
    live::{LiveArpeggiator, OutputEvent, OutputKind},
};
use coremidi::{Client, Destination, Destinations, PacketBuffer, Source, Sources};

use crate::HeldProfile;

pub fn list_ports() {
    println!("Sources:");
    for index in 0..Sources::count() {
        let source = Source::from_index(index).expect("listed MIDI source");
        println!("  {index}: {}", source.display_name().unwrap_or_default());
    }
    println!("Destinations:");
    for index in 0..Destinations::count() {
        let destination = Destination::from_index(index).expect("listed MIDI destination");
        println!(
            "  {index}: {}",
            destination.display_name().unwrap_or_default()
        );
    }
}

pub fn play(
    profile: HeldProfile,
    source_index: usize,
    destination_index: usize,
    bpm: u32,
) -> Result<(), String> {
    if bpm == 0 || bpm > 1000 {
        return Err("BPM must be between 1 and 1000".into());
    }
    let source = Source::from_index(source_index).ok_or("MIDI source index is unavailable")?;
    let destination = Destination::from_index(destination_index)
        .ok_or("MIDI destination index is unavailable")?;
    let client = Client::new("arpeg").map_err(|e| format!("CoreMIDI client: {e}"))?;
    let output_port = client
        .output_port("arpeg output")
        .map_err(|e| format!("CoreMIDI output: {e}"))?;
    let (sender, receiver) = mpsc::channel();
    let input_port = client
        .input_port("arpeg input", move |packets| {
            for packet in packets.iter() {
                let _ = sender.send((Instant::now(), packet.data().to_vec()));
            }
        })
        .map_err(|e| format!("CoreMIDI input: {e}"))?;
    input_port
        .connect_source(&source)
        .map_err(|e| format!("connect MIDI source: {e}"))?;
    let mut arp = LiveArpeggiator::new(
        profile.bank,
        profile.selection,
        profile.step,
        profile.gate,
        profile.retrigger,
    )
    .map_err(str::to_owned)?;
    let stopped = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&stopped);
    ctrlc::set_handler(move || signal.store(true, Ordering::SeqCst)).map_err(|e| e.to_string())?;
    let keyboard = Arc::clone(&stopped);
    let (command_sender, command_receiver) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match io::stdin().read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) if line.trim() == "clear" => {
                    if command_sender.send(()).is_err() {
                        break;
                    }
                }
                Ok(_) if line.trim().is_empty() || line.trim() == "quit" => {
                    keyboard.store(true, Ordering::SeqCst);
                    break;
                }
                Ok(_) => eprintln!("enter clear, quit, or an empty line"),
            }
        }
    });
    println!(
        "Playing arpeggio at {bpm} BPM. Enter clear for a latch, or press Enter or Ctrl-C to stop."
    );
    let mut start = None;
    let mut parser = MidiNotes::default();
    let mut last = Beat::from_integer(0);
    let result = (|| -> Result<(), String> {
        while !stopped.load(Ordering::SeqCst) {
            while let Ok((arrival, data)) = receiver.try_recv() {
                let notes = parser.feed(&data);
                if notes.is_empty() {
                    continue;
                }
                let origin = *start.get_or_insert(arrival);
                let at = elapsed_beat(origin, arrival, bpm).max(last);
                last = at;
                for note in notes {
                    let events = match note {
                        NoteInput::On(key, velocity) => arp.note_on(at, key, velocity),
                        NoteInput::Off(key) => arp.note_off(at, key),
                    }
                    .map_err(str::to_owned)?;
                    send_events(&output_port, &destination, &events)?;
                }
            }
            while command_receiver.try_recv().is_ok() {
                if profile.bank == Bank::Held {
                    eprintln!("clear requires a latched bank");
                    continue;
                }
                let at = start.map_or(last, |origin| {
                    elapsed_beat(origin, Instant::now(), bpm).max(last)
                });
                last = at;
                send_events(
                    &output_port,
                    &destination,
                    &arp.clear(at).map_err(str::to_owned)?,
                )?;
            }
            if let Some(origin) = start {
                let now = elapsed_beat(origin, Instant::now(), bpm).max(last);
                last = now;
                send_events(
                    &output_port,
                    &destination,
                    &arp.advance(now).map_err(str::to_owned)?,
                )?;
            }
            thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    })();
    let at = start.map_or(last, |origin| {
        elapsed_beat(origin, Instant::now(), bpm).max(last)
    });
    if let Ok(events) = arp.stop(at) {
        let _ = send_events(&output_port, &destination, &events);
    }
    result
}

fn elapsed_beat(start: Instant, now: Instant, bpm: u32) -> Beat {
    let micros =
        i64::try_from(now.saturating_duration_since(start).as_micros()).unwrap_or(i64::MAX);
    Beat::new(micros, 60_000_000) * i64::from(bpm)
}

fn send_events(
    port: &coremidi::OutputPort,
    destination: &Destination,
    events: &[OutputEvent],
) -> Result<(), String> {
    for event in events {
        let bytes = match event.kind {
            OutputKind::NoteOn { key, velocity, .. } => [0x90, key, velocity],
            OutputKind::NoteOff { key, .. } => [0x80, key, 0],
        };
        port.send(destination, &PacketBuffer::new(0, &bytes))
            .map_err(|e| format!("CoreMIDI send: {e}"))?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NoteInput {
    On(u8, u8),
    Off(u8),
}

#[derive(Default)]
struct MidiNotes {
    status: u8,
    first: Option<u8>,
    sysex: bool,
}

impl MidiNotes {
    fn feed(&mut self, bytes: &[u8]) -> Vec<NoteInput> {
        let mut notes = Vec::new();
        for &byte in bytes {
            if byte >= 0xf8 {
                continue;
            }
            if byte == 0xf0 {
                self.sysex = true;
                self.status = 0;
                self.first = None;
                continue;
            }
            if byte == 0xf7 {
                self.sysex = false;
                continue;
            }
            if self.sysex {
                continue;
            }
            if byte >= 0x80 {
                self.status = if byte < 0xf0 { byte } else { 0 };
                self.first = None;
                continue;
            }
            if self.status == 0 {
                continue;
            }
            let kind = self.status & 0xf0;
            if matches!(kind, 0xc0 | 0xd0) {
                continue;
            }
            if let Some(first) = self.first.take() {
                if self.status & 0x0f == 0 {
                    match kind {
                        0x90 if byte > 0 => notes.push(NoteInput::On(first, byte)),
                        0x80 | 0x90 => notes.push(NoteInput::Off(first)),
                        _ => {}
                    }
                }
            } else {
                self.first = Some(byte);
            }
        }
        notes
    }
}

#[cfg(test)]
mod tests {
    use super::{MidiNotes, NoteInput};

    #[test]
    fn decodes_running_status_and_velocity_zero_releases() {
        let mut parser = MidiNotes::default();
        assert_eq!(parser.feed(&[0x90, 60]), []);
        assert_eq!(
            parser.feed(&[100, 64, 90, 60, 0]),
            [
                NoteInput::On(60, 100),
                NoteInput::On(64, 90),
                NoteInput::Off(60)
            ]
        );
    }
}
