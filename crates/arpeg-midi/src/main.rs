use std::{env, fs, process};

use arpeg_midi::{parse_profile, render_file};

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
        _ => {
            return Err(
                "usage: arpeg validate PROFILE | arpeg render-file PROFILE INPUT.mid OUTPUT.mid"
                    .into(),
            );
        }
    }
    Ok(())
}
