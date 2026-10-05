//! A small CoreMIDI host for classic and recorded-history presets.

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
    capture::{MidiEvent, Profile as CaptureProfile},
    gesture::{RealizedEvent, Tick},
    history::HistoryArpeggiator,
    live::{LiveArpeggiator, OutputEvent, OutputKind},
};
use coremidi::{Client, Destination, Destinations, PacketBuffer, Source, Sources};

use crate::{HeldProfile, HistoryProfile, Profile};

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
    profile: Profile,
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
    match profile {
        Profile::Classic(profile) => run_classic(
            profile,
            bpm,
            &receiver,
            &command_receiver,
            &output_port,
            &destination,
            &stopped,
        ),
        Profile::History(profile) => run_history(
            profile,
            bpm,
            &receiver,
            &command_receiver,
            &output_port,
            &destination,
            &stopped,
        ),
    }
}

fn run_classic(
    profile: HeldProfile,
    bpm: u32,
    receiver: &mpsc::Receiver<(Instant, Vec<u8>)>,
    commands: &mpsc::Receiver<()>,
    output_port: &coremidi::OutputPort,
    destination: &Destination,
    stopped: &AtomicBool,
) -> Result<(), String> {
    let mut arp = LiveArpeggiator::new(
        profile.bank,
        profile.selection,
        profile.rhythm,
        profile.gate,
        profile.retrigger,
        profile.chance,
    )
    .map_err(str::to_owned)?;
    println!(
        "Playing arpeggio at {bpm} BPM. Enter clear for a latch, or press Enter or Ctrl-C to stop."
    );
    let mut start = None;
    let mut parser = MidiMessages::default();
    let mut last = Beat::from_integer(0);
    let result = (|| -> Result<(), String> {
        while !stopped.load(Ordering::SeqCst) {
            while let Ok((arrival, data)) = receiver.try_recv() {
                let notes: Vec<_> = parser
                    .feed(&data)
                    .iter()
                    .filter_map(|message| NoteInput::decode(message))
                    .collect();
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
                    send_events(output_port, destination, &events)?;
                }
            }
            while commands.try_recv().is_ok() {
                if profile.bank == Bank::Held {
                    eprintln!("clear requires a latched bank");
                    continue;
                }
                let at = start.map_or(last, |origin| {
                    elapsed_beat(origin, Instant::now(), bpm).max(last)
                });
                last = at;
                send_events(
                    output_port,
                    destination,
                    &arp.clear(at).map_err(str::to_owned)?,
                )?;
            }
            if let Some(origin) = start {
                let now = elapsed_beat(origin, Instant::now(), bpm).max(last);
                last = now;
                send_events(
                    output_port,
                    destination,
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
        let _ = send_events(output_port, destination, &events);
    }
    result
}

fn run_history(
    profile: HistoryProfile,
    bpm: u32,
    receiver: &mpsc::Receiver<(Instant, Vec<u8>)>,
    commands: &mpsc::Receiver<()>,
    output_port: &coremidi::OutputPort,
    destination: &Destination,
    stopped: &AtomicBool,
) -> Result<(), String> {
    let step = profile.step * Tick::new(60_000_000, i64::from(bpm));
    let mut arp = HistoryArpeggiator::new(
        profile.notes,
        profile.selection,
        profile.retrigger == arpeg_core::live::Retrigger::BankEdit,
        step,
        profile.gate,
        CaptureProfile::default(),
    )
    .map_err(str::to_owned)?;
    println!(
        "Playing recorded note history on MIDI channel 1 at {bpm} BPM. Enter clear, or press Enter or Ctrl-C to stop."
    );
    let mut start = None;
    let mut parser = MidiMessages::default();
    let mut last_published = -1i64;
    let mut last_input = -1i64;
    let mut ordinal = 0u32;
    let mut reported_late = false;
    let result = (|| -> Result<(), String> {
        while !stopped.load(Ordering::SeqCst) {
            while let Ok((arrival, data)) = receiver.try_recv() {
                let messages = parser.feed(&data);
                if messages.is_empty() {
                    continue;
                }
                let origin = *start.get_or_insert(arrival);
                let source_tick = elapsed_micros(origin, arrival);
                let tick = source_tick
                    .max(last_published.saturating_add(1))
                    .max(last_input);
                if tick != source_tick && !reported_late {
                    eprintln!("late MIDI input moved to the next capture tick");
                    reported_late = true;
                }
                send_realized(
                    output_port,
                    destination,
                    &arp.before(tick).map_err(str::to_owned)?,
                )?;
                if tick != last_input {
                    ordinal = 0;
                    last_input = tick;
                }
                for data in messages {
                    arp.accept(MidiEvent {
                        tick,
                        ordinal,
                        data,
                    })
                    .map_err(str::to_owned)?;
                    ordinal = ordinal
                        .checked_add(1)
                        .ok_or("too many simultaneous MIDI messages")?;
                }
            }
            while commands.try_recv().is_ok() {
                let tick = start
                    .map_or(0, |origin| elapsed_micros(origin, Instant::now()))
                    .max(last_published);
                send_realized(
                    output_port,
                    destination,
                    &arp.clear(tick).map_err(str::to_owned)?,
                )?;
            }
            if let Some(origin) = start {
                let now = elapsed_micros(origin, Instant::now()).max(last_published);
                send_realized(
                    output_port,
                    destination,
                    &arp.advance(now).map_err(str::to_owned)?,
                )?;
                last_published = now;
            }
            thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    })();
    let at = start
        .map_or(0, |origin| elapsed_micros(origin, Instant::now()))
        .max(last_published);
    if let Ok(events) = arp.clear(at) {
        let _ = send_realized(output_port, destination, &events);
    }
    result
}

fn elapsed_micros(start: Instant, now: Instant) -> i64 {
    i64::try_from(now.saturating_duration_since(start).as_micros()).unwrap_or(i64::MAX)
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

fn send_realized(
    port: &coremidi::OutputPort,
    destination: &Destination,
    events: &[RealizedEvent],
) -> Result<(), String> {
    for event in events {
        port.send(destination, &PacketBuffer::new(0, &event.data))
            .map_err(|error| format!("CoreMIDI send: {error}"))?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NoteInput {
    On(u8, u8),
    Off(u8),
}

impl NoteInput {
    fn decode(message: &[u8]) -> Option<Self> {
        if message.len() != 3 || message[0] & 0x0f != 0 {
            return None;
        }
        match message[0] & 0xf0 {
            0x90 if message[2] > 0 => Some(Self::On(message[1], message[2])),
            0x80 | 0x90 => Some(Self::Off(message[1])),
            _ => None,
        }
    }
}

#[derive(Default)]
struct MidiMessages {
    status: u8,
    data: Vec<u8>,
    sysex: Option<Vec<u8>>,
}

impl MidiMessages {
    fn feed(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut messages = Vec::new();
        for &byte in bytes {
            if byte >= 0xf8 {
                messages.push(vec![byte]);
                continue;
            }
            if let Some(sysex) = &mut self.sysex {
                sysex.push(byte);
                if byte == 0xf7 {
                    messages.push(self.sysex.take().expect("active SysEx"));
                }
                continue;
            }
            if byte == 0xf0 {
                self.sysex = Some(vec![byte]);
                self.status = 0;
                self.data.clear();
                continue;
            }
            if byte >= 0x80 {
                self.status = match byte {
                    0x80..=0xef | 0xf1..=0xf3 => byte,
                    _ => 0,
                };
                self.data.clear();
                if self.status == 0 {
                    messages.push(vec![byte]);
                }
                continue;
            }
            if self.status == 0 {
                continue;
            }
            self.data.push(byte);
            let needed = match self.status {
                0xc0..=0xdf | 0xf1 | 0xf3 => 1,
                _ => 2,
            };
            if self.data.len() == needed {
                messages.push([&[self.status], self.data.as_slice()].concat());
                self.data.clear();
                if self.status >= 0xf0 {
                    self.status = 0;
                }
            }
        }
        messages
    }
}

#[cfg(test)]
mod tests {
    use super::{MidiMessages, NoteInput};

    #[test]
    fn decodes_running_status_and_velocity_zero_releases() {
        let mut parser = MidiMessages::default();
        assert!(parser.feed(&[0x90, 60]).is_empty());
        let notes: Vec<_> = parser
            .feed(&[100, 64, 90, 60, 0])
            .iter()
            .filter_map(|message| NoteInput::decode(message))
            .collect();
        assert_eq!(
            notes,
            [
                NoteInput::On(60, 100),
                NoteInput::On(64, 90),
                NoteInput::Off(60)
            ]
        );
    }

    #[test]
    fn keeps_controller_bend_system_and_realtime_messages() {
        let mut parser = MidiMessages::default();
        assert_eq!(
            parser.feed(&[0xb2, 2, 13, 0xe2, 64, 81]),
            [vec![0xb2, 2, 13], vec![0xe2, 64, 81]]
        );
        assert_eq!(
            parser.feed(&[0xf8, 0xc2, 7, 0xf0, 1]),
            [vec![0xf8], vec![0xc2, 7]]
        );
        assert_eq!(
            parser.feed(&[0xf8, 2, 0xf7]),
            [vec![0xf8], vec![0xf0, 1, 2, 0xf7]]
        );
    }
}
