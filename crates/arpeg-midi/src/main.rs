use std::{env, fs, process};

use arpeg_midi::{parse_profile, render_file};

use arpeg_core::clock::ClockMode;
use arpeg_midi::live::{list_ports, play};

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
            parse_profile(&text)?;
            println!("supported arpeggiator profile");
        }
        [_, command, profile, input, output] if command == "render-file" => {
            let text = fs::read_to_string(profile).map_err(|e| e.to_string())?;
            let input = fs::read(input).map_err(|e| e.to_string())?;
            let rendered = render_file(&text, &input)?;
            fs::write(output, rendered).map_err(|e| e.to_string())?;
        }
        [_, command] if command == "list-ports" => list_ports()?,
        [_, command, profile, source, destination, bpm, options @ ..] if command == "play" => {
            let text = fs::read_to_string(profile).map_err(|e| e.to_string())?;
            let profile = parse_profile(&text)?;
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
                mode,
                clock_source,
                timeout_us,
            )?;
        }
        _ => {
            return Err(
                "usage: arpeg validate PROFILE | arpeg render-file PROFILE INPUT.mid OUTPUT.mid | arpeg list-ports | arpeg play PROFILE SOURCE_INDEX DESTINATION_INDEX BPM [--clock internal|external] [--clock-source INDEX] [--clock-timeout-ms MS]"
                    .into(),
            );
        }
    }
    Ok(())
}
