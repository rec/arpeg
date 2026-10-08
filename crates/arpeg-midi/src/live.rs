//! Portable MIDI host using the device-independent transport player.

use crate::{
    Profile,
    player::{InputSource, MidiPlayer},
};
use arpeg_core::{Bank, clock::ClockMode, ports::InputPort};
use midir::{Ignore, MidiInput, MidiOutput, MidiOutputConnection};
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

pub fn list_ports() -> Result<(), String> {
    let input = MidiInput::new("arpeg input").map_err(|e| format!("MIDI input: {e}"))?;
    let output = MidiOutput::new("arpeg output").map_err(|e| format!("MIDI output: {e}"))?;
    println!("Sources:");
    for (index, port) in input.ports().iter().enumerate() {
        let name = input
            .port_name(port)
            .map_err(|e| format!("MIDI source name: {e}"))?;
        println!("  {index}: {name}");
    }
    println!("Destinations:");
    for (index, port) in output.ports().iter().enumerate() {
        let name = output
            .port_name(port)
            .map_err(|e| format!("MIDI destination name: {e}"))?;
        println!("  {index}: {name}");
    }
    Ok(())
}

pub fn play(
    profile: Profile,
    source_index: usize,
    destination_index: usize,
    bpm: u32,
    mode: ClockMode,
    clock_source: Option<usize>,
    timeout_us: i64,
) -> Result<(), String> {
    if clock_source.is_some() && mode != ClockMode::External {
        return Err("clock source requires external clock".into());
    }
    let held = matches!(&profile, Profile::Classic(p) if p.bank == Bank::Held);
    let mut player = MidiPlayer::new(profile, mode, bpm, timeout_us).map_err(str::to_owned)?;
    let mut input = MidiInput::new("arpeg input").map_err(|e| format!("MIDI input: {e}"))?;
    input.ignore(Ignore::None);
    let output = MidiOutput::new("arpeg output").map_err(|e| format!("MIDI output: {e}"))?;
    let sources = input.ports();
    let source = sources
        .get(source_index)
        .ok_or("MIDI source index is unavailable")?;
    if clock_source.is_some_and(|i| i >= sources.len()) {
        return Err("MIDI clock source index is unavailable".into());
    }
    let destinations = output.ports();
    let destination = destinations
        .get(destination_index)
        .ok_or("MIDI destination index is unavailable")?;
    let separate = clock_source.is_some_and(|i| i != source_index);
    let (sender, receiver) = mpsc::channel();
    let clock_sender = sender.clone();
    let _input_connection = input
        .connect(
            source,
            "arpeg input",
            move |_timestamp, data, sender| {
                let source = if separate {
                    InputSource::Notes
                } else {
                    InputSource::Both
                };
                let _ = sender.send((Instant::now(), data.to_vec(), source));
            },
            sender,
        )
        .map_err(|e| format!("connect MIDI source: {e}"))?;
    let _clock_connection = if separate {
        let mut input =
            MidiInput::new("arpeg clock").map_err(|e| format!("MIDI clock input: {e}"))?;
        input.ignore(Ignore::None);
        let ports = input.ports();
        let port = ports
            .get(clock_source.expect("separate clock input"))
            .ok_or("MIDI clock source index is unavailable")?;
        Some(
            input
                .connect(
                    port,
                    "arpeg clock",
                    move |_timestamp, data, sender| {
                        let _ = sender.send((Instant::now(), data.to_vec(), InputSource::Clock));
                    },
                    clock_sender,
                )
                .map_err(|e| format!("connect MIDI clock: {e}"))?,
        )
    } else {
        None
    };
    let mut output = output
        .connect(destination, "arpeg output")
        .map_err(|e| format!("connect MIDI destination: {e}"))?;
    let stopped = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&stopped);
    ctrlc::set_handler(move || signal.store(true, Ordering::SeqCst)).map_err(|e| e.to_string())?;
    let keyboard = Arc::clone(&stopped);
    let (command_sender, commands) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            if io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
                break;
            }
            let command = line.trim();
            if command.is_empty() || command == "quit" {
                break;
            }
            if command_sender.send(command.to_owned()).is_err() {
                return;
            }
        }
        keyboard.store(true, Ordering::SeqCst);
    });
    println!(
        "Playing with {mode:?} clock. Enter start, pause, continue, tempo BPM, gate FRACTION, density FRACTION, transposition SEMITONES, selection_offset RANKS, breath FRACTION, bend FRACTION, pressure FRACTION, record, commit, overdub, undo, clear, or quit."
    );
    let origin = Instant::now();
    let elapsed = |now: Instant| {
        i64::try_from(now.saturating_duration_since(origin).as_micros()).unwrap_or(i64::MAX)
    };
    let mut notes = MidiMessages::default();
    let mut clocks = MidiMessages::default();
    let result = (|| -> Result<(), String> {
        while !stopped.load(Ordering::SeqCst) {
            while let Ok((arrival, data, source)) = receiver.try_recv() {
                let parser = if matches!(source, InputSource::Clock) {
                    &mut clocks
                } else {
                    &mut notes
                };
                for data in parser.feed(&data) {
                    send(
                        &mut output,
                        player
                            .accept(elapsed(arrival), &data, source)
                            .map_err(str::to_owned)?,
                    )?;
                }
            }
            while let Ok(command) = commands.try_recv() {
                let at = elapsed(Instant::now());
                let messages = match command.as_str() {
                    "start" => player.accept(at, &[0xfa], InputSource::Both),
                    "pause" => player.accept(at, &[0xfc], InputSource::Both),
                    "continue" => player.accept(at, &[0xfb], InputSource::Both),
                    "record" | "commit" | "overdub" | "undo" => match player.capture(at, &command) {
                        Ok(messages) => {
                            if let Some((recording, notes, revision)) = player.capture_state() {
                                println!("{command}: recording={recording}, notes={notes}, revision={revision}. Committed edits publish at the next step.");
                            }
                            Ok(messages)
                        }
                        Err(error) => {
                            eprintln!("{error}");
                            continue;
                        }
                    },
                    "clear" if !held => player.clear(at),
                    "clear" => {
                        eprintln!("clear requires a latched, history or phrase bank");
                        continue;
                    }
                    _ => {
                        if let Some((name, value)) = command.split_once(' ') {
                            let port = match name { "gate" => Some(InputPort::Gate), "density" => Some(InputPort::Density), "transposition" => Some(InputPort::Transposition), "selection_offset" => Some(InputPort::SelectionOffset), "breath" => Some(InputPort::Breath), "bend" => Some(InputPort::Bend), "pressure" => Some(InputPort::Pressure), _ => None };
                            if let Some(port) = port {
                                match value.parse() {
                                    Ok(value) => match player.control(at, port, value) {
                                        Ok(messages) => send(&mut output, messages)?,
                                        Err(error) => eprintln!("{error}"),
                                    },
                                    Err(_) => eprintln!("controls require an exact rational value; transposition and selection_offset require whole numbers"),
                                }
                                continue;
                            }
                        }
                        if let Some(bpm) =
                            command.strip_prefix("tempo ").and_then(|s| s.parse().ok())
                        {
                            match player.set_tempo(at, bpm) {
                                Ok(messages) => Ok(messages),
                                Err(error) => {
                                    eprintln!("{error}");
                                    continue;
                                }
                            }
                        } else {
                            eprintln!("enter start, pause, continue, tempo BPM, gate FRACTION, density FRACTION, transposition SEMITONES, selection_offset RANKS, breath FRACTION, bend FRACTION, pressure FRACTION, record, commit, overdub, undo, clear, or quit");
                            continue;
                        }
                    }
                }
                .map_err(str::to_owned)?;
                send(&mut output, messages)?;
            }
            send(
                &mut output,
                player
                    .advance(elapsed(Instant::now()))
                    .map_err(str::to_owned)?,
            )?;
            if player.take_events().exhausted {
                eprintln!("Motion output event buffer exhausted; skipped new steps");
            }
            thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    })();
    if let Ok(messages) = player.stop(elapsed(Instant::now())) {
        let _ = send(&mut output, messages);
    }
    result
}

fn send(port: &mut MidiOutputConnection, messages: Vec<Vec<u8>>) -> Result<(), String> {
    for data in messages {
        port.send(&data).map_err(|e| format!("MIDI send: {e}"))?;
    }
    Ok(())
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
    use super::MidiMessages;

    #[test]
    fn decodes_running_status_and_velocity_zero_releases() {
        let mut parser = MidiMessages::default();
        assert!(parser.feed(&[0x90, 60]).is_empty());
        assert_eq!(
            parser.feed(&[100, 64, 90, 60, 0]),
            [vec![0x90, 60, 100], vec![0x90, 64, 90], vec![0x90, 60, 0]]
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
