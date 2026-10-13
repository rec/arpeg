//! Synthetic live-player timing and cleanup checks, without MIDI ports.

use arpeg_core::{clock::ClockMode, ports::NotePort};
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};
use std::{collections::BTreeMap, thread::sleep, time::Duration, time::Instant};

fn main() {
    for (rate, stall_ms) in [(1000, 0), (4000, 0), (1000, 200)] {
        check(rate, stall_ms);
    }
}

fn check(rate: i64, stall_ms: u64) {
    let profile = parse_profile(include_str!("../../../conformance/live-wind.toml"), None).unwrap();
    let mut player = MidiPlayer::new(profile, ClockMode::Internal, 120, 500_000).unwrap();
    let interval = 1_000_000 / rate;
    let mut next_input = interval;
    let mut samples = Vec::new();
    let mut lateness = Vec::new();
    let mut owned = BTreeMap::new();
    let mut attacks = 0;
    let mut releases = 0;
    let mut wire_bytes = 0;
    let mut stalled = false;
    let origin = Instant::now();
    player
        .accept(0, &[144, 60, 100], InputSource::Both)
        .unwrap();
    loop {
        let now = origin.elapsed().as_micros() as i64;
        if now >= 2_000_000 {
            break;
        }
        if !stalled && stall_ms > 0 && now >= 1_000_000 {
            sleep(Duration::from_millis(stall_ms));
            stalled = true;
            continue;
        }
        let started = Instant::now();
        while next_input <= now {
            let output = player
                .accept(next_input, &[176, 2, 64], InputSource::Both)
                .unwrap();
            wire_bytes += output.iter().map(Vec::len).sum::<usize>();
            next_input += interval;
        }
        let output = player.advance(now).unwrap();
        wire_bytes += output.iter().map(Vec::len).sum::<usize>();
        let batch = player.take_events();
        assert!(!batch.exhausted);
        let delivered = origin.elapsed().as_micros() as i64;
        for event in batch.notes {
            let due = event.at * 500_000;
            lateness.push((delivered - due.to_integer()).max(0));
            match event.port {
                NotePort::NoteStart => {
                    owned.insert(event.occurrence, event.key);
                    attacks += 1;
                }
                NotePort::NoteEnd => {
                    assert_eq!(owned.remove(&event.occurrence), Some(event.key));
                    releases += 1;
                }
            }
        }
        samples.push(started.elapsed().as_micros());
        sleep(Duration::from_millis(1));
    }
    wire_bytes += player
        .stop(origin.elapsed().as_micros() as i64)
        .unwrap()
        .iter()
        .map(Vec::len)
        .sum::<usize>();
    for event in player.take_events().notes {
        assert_eq!(event.port, NotePort::NoteEnd);
        assert_eq!(owned.remove(&event.occurrence), Some(event.key));
        releases += 1;
    }
    assert!(owned.is_empty());
    assert_eq!(attacks, releases);
    let elapsed = origin.elapsed().as_secs_f64();
    samples.sort_unstable();
    lateness.sort_unstable();
    println!(
        "runtime=rust controller_hz={rate} stall_ms={stall_ms} elapsed_ms={:.3} polls={} batch_p99_us={} batch_max_us={} note_lateness_p99_us={} note_lateness_max_us={} wire_bytes_per_second={:.3} attacks={attacks} releases={releases} owned_after_stop={}",
        elapsed * 1000.0,
        samples.len(),
        samples[(samples.len() * 99).div_ceil(100) - 1],
        samples.last().unwrap(),
        lateness[(lateness.len() * 99).div_ceil(100) - 1],
        lateness.last().unwrap(),
        wire_bytes as f64 / elapsed,
        owned.len()
    );
}
