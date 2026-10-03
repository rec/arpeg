use std::{env, fs, process};

use arpeg_midi::{Profile, parse_profile, render_file};

#[cfg(target_os = "macos")]
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
        #[cfg(target_os = "macos")]
        [_, command] if command == "list-ports" => list_ports(),
        #[cfg(target_os = "macos")]
        [_, command, profile, source, destination, bpm] if command == "play" => {
            let text = fs::read_to_string(profile).map_err(|e| e.to_string())?;
            let profile = parse_profile(&text)?;
            let source = source
                .parse()
                .map_err(|_| "source index must be an integer")?;
            let destination = destination
                .parse()
                .map_err(|_| "destination index must be an integer")?;
            let bpm = bpm.parse().map_err(|_| "BPM must be a positive integer")?;
            match profile {
                Profile::Classic(profile) => play(profile, source, destination, bpm)?,
                Profile::History(_) => return Err("history live playback is not wired yet".into()),
            }
        }
        _ => {
            return Err(
                "usage: arpeg validate PROFILE | arpeg render-file PROFILE INPUT.mid OUTPUT.mid | arpeg list-ports | arpeg play PROFILE SOURCE_INDEX DESTINATION_INDEX BPM"
                    .into(),
            );
        }
    }
    Ok(())
}
