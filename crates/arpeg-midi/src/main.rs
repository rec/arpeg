use std::path::Path;
use std::{env, fs, process};

use arpeg_midi::{parse_profile, render_file};

use arpeg_core::clock::ClockMode;
use arpeg_midi::input::InputLimits;
use arpeg_midi::live::{PlayOptions, list_ports, play};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<_> = env::args().collect();
    match args.as_slice() {
        [_, command, profile] if command == "validate" => {
            let text = fs::read_to_string(profile).map_err(|e| e.to_string())?;
            parse_profile(&text, Some(Path::new(profile)))?;
            println!("supported arpeggiator profile");
        }
        [_, command, profile, input, output] if command == "render-file" => {
            let text = fs::read_to_string(profile).map_err(|e| e.to_string())?;
            let input = fs::read(input).map_err(|e| e.to_string())?;
            let rendered = render_file(&text, &input, Some(Path::new(profile)))?;
            fs::write(output, rendered).map_err(|e| e.to_string())?;
        }
        [_, command] if command == "list-ports" => list_ports()?,
        [_, command, profile, source, destination, bpm, options @ ..] if command == "play" => {
            let text = fs::read_to_string(profile).map_err(|e| e.to_string())?;
            let profile = parse_profile(&text, Some(Path::new(profile)))?;
            let source = source
                .parse()
                .map_err(|_| "source index must be an integer")?;
            let destination = destination
                .parse()
                .map_err(|_| "destination index must be an integer")?;
            let bpm = bpm.parse().map_err(|_| "BPM must be a positive integer")?;
            let mut mode = ClockMode::Internal;
            let mut clock_source = None;
            let mut timeout_us = 500_000;
            let mut input = InputLimits::default();
            let mut output_bytes_per_second = None;
            let mut output_burst_bytes = 64;
            let mut pairs = options.chunks_exact(2);
            for pair in &mut pairs {
                match pair[0].as_str() {
                    "--clock" => {
                        mode = match pair[1].as_str() {
                            "internal" => ClockMode::Internal,
                            "external" => ClockMode::External,
                            _ => return Err("clock must be internal or external".into()),
                        }
                    }
                    "--clock-source" => {
                        clock_source = Some(
                            pair[1]
                                .parse()
                                .map_err(|_| "clock source must be an index")?,
                        )
                    }
                    "--clock-timeout-ms" => {
                        timeout_us = pair[1]
                            .parse::<i64>()
                            .ok()
                            .and_then(|t| t.checked_mul(1000))
                            .ok_or("invalid clock timeout")?
                    }
                    "--input-packets" | "--input-bytes" | "--max-message-bytes" => {
                        let value = pair[1]
                            .parse::<usize>()
                            .ok()
                            .filter(|v| *v > 0)
                            .ok_or("input limits must be positive integers")?;
                        match pair[0].as_str() {
                            "--input-packets" => input.packets = value,
                            "--input-bytes" => input.bytes = value,
                            _ => input.max_message_bytes = value,
                        }
                    }
                    "--output-bytes-per-second" => {
                        output_bytes_per_second = Some(
                            pair[1]
                                .parse::<u64>()
                                .ok()
                                .filter(|v| *v > 0)
                                .ok_or("output byte rate must be a positive integer")?,
                        );
                    }
                    "--output-burst-bytes" => {
                        output_burst_bytes = pair[1]
                            .parse::<usize>()
                            .ok()
                            .filter(|v| *v > 0)
                            .ok_or("output burst allowance must be a positive integer")?;
                    }
                    _ => return Err(format!("unknown play option: {}", pair[0])),
                }
            }
            if !pairs.remainder().is_empty() {
                return Err("play options require a value".into());
            }
            play(
                profile,
                source,
                destination,
                bpm,
                PlayOptions {
                    mode,
                    clock_source,
                    timeout_us,
                    input,
                    output_bytes_per_second,
                    output_burst_bytes,
                },
            )?;
        }
        _ => {
            return Err(
                "usage: arpeg validate PROFILE | arpeg render-file PROFILE INPUT.mid OUTPUT.mid | arpeg list-ports | arpeg play PROFILE SOURCE_INDEX DESTINATION_INDEX BPM [--clock internal|external] [--clock-source INDEX] [--clock-timeout-ms MS] [--input-packets COUNT] [--input-bytes BYTES] [--max-message-bytes BYTES] [--output-bytes-per-second BYTES] [--output-burst-bytes BYTES]"
                    .into(),
            );
        }
    }
    Ok(())
}
