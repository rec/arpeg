//! Opt-in CoreMIDI qualification through the actual arpeg executable.
#![cfg(all(feature = "device-host", target_os = "macos"))]

use meedee::transport::{Input, InputOptions, Output, OutputConnection, OutputOptions, Packet};
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::process::CommandExt,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

struct Host(Child);

impl Host {
    fn start(source: usize, destination: usize, clock: Option<usize>, overflow: bool) -> Self {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut command = Command::new(env!("CARGO_BIN_EXE_arpeg"));
        command.current_dir(root).args([
            "play",
            "conformance/up.toml",
            &source.to_string(),
            &destination.to_string(),
            "120",
        ]);
        if let Some(clock) = clock {
            command.args(["--clock", "external", "--clock-source", &clock.to_string()]);
        }
        if overflow {
            command.args(["--input-bytes", "2"]);
        }
        let mut host = Self(
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = host.0.stdout.take().unwrap();
        let (sender, receiver) = mpsc::sync_channel(16);
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let _ = sender.try_send(line.unwrap());
            }
        });
        let ready = receiver.recv_timeout(Duration::from_secs(5));
        assert!(
            ready
                .as_ref()
                .is_ok_and(|line| line.starts_with("Playing with")),
            "arpeg did not become ready: {ready:?}"
        );
        host
    }

    fn finish(&mut self, quit: bool) -> (bool, String) {
        if quit {
            self.0.stdin.as_mut().unwrap().write_all(b"quit\n").unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "arpeg did not stop within five seconds"
            );
            thread::sleep(Duration::from_millis(5));
        };
        let mut stderr = String::new();
        self.0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        (status.success(), stderr)
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn port_index(listing: &str, section: &str, name: &str) -> usize {
    listing
        .split_once(section)
        .unwrap()
        .1
        .lines()
        .find_map(|line| {
            let (index, found) = line.trim().split_once(": ")?;
            (found == name).then(|| index.parse().unwrap())
        })
        .expect("test-owned endpoint was not listed")
}

fn send(port: &OutputConnection, data: &[u8]) {
    port.send(&Packet::Bytes(data.to_vec())).unwrap();
}

fn receive_note(receiver: &Receiver<Packet>, status: u8) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let Packet::Bytes(data) = receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap()
        else {
            panic!("unexpected UMP");
        };
        if data == [status, 60, if status == 0x90 { 100 } else { 0 }] {
            return;
        }
    }
}

#[test]
#[ignore = "creates only test-owned virtual endpoints; requires working CoreMIDI"]
fn executable_delivers_notes_cleans_up_and_reports_transport_loss() {
    // Supervise the whole native flow, including SDK calls and the CLI children.
    if std::env::var_os("ARPEG_NATIVE_HOST_CHILD").is_none() {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "executable_delivers_notes_cleans_up_and_reports_transport_loss",
                "--nocapture",
            ])
            .env("ARPEG_NATIVE_HOST_CHILD", "1")
            .process_group(0)
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "native host check failed: {status}");
                return;
            }
            if Instant::now() >= deadline {
                let _ = Command::new("/bin/kill")
                    .args(["-KILL", &format!("-{}", child.id())])
                    .status();
                let _ = child.kill();
                let _ = child.wait();
                panic!("native host check exceeded its 30-second deadline");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
    let prefix = format!("arpeg-transport-{}", std::process::id());
    let source_name = format!("{prefix}-source");
    let clock_name = format!("{prefix}-clock");
    let destination_name = format!("{prefix}-destination");
    let source = Output::new(OutputOptions::default())
        .unwrap()
        .create_virtual(&source_name)
        .unwrap();
    let clock = Output::new(OutputOptions::default())
        .unwrap()
        .create_virtual(&clock_name)
        .unwrap();
    let (sender, receiver) = mpsc::sync_channel(128);
    let destination = Input::new(InputOptions::default())
        .unwrap()
        .create_virtual(&destination_name, move |packet| {
            sender
                .try_send(packet.packet)
                .expect("native test receive overflow");
        })
        .unwrap();
    let listing = Command::new(env!("CARGO_BIN_EXE_arpeg"))
        .arg("list-ports")
        .output()
        .unwrap();
    assert!(
        listing.status.success(),
        "{}",
        String::from_utf8_lossy(&listing.stderr)
    );
    let listing = String::from_utf8(listing.stdout).unwrap();
    let source_index = port_index(&listing, "Sources:", &source_name);
    let clock_index = port_index(&listing, "Sources:", &clock_name);
    let destination_index = port_index(&listing, "Destinations:", &destination_name);

    for external in [None, Some(source_index), Some(clock_index)] {
        while receiver.try_recv().is_ok() {}
        let mut host = Host::start(source_index, destination_index, external, false);
        send(&source, &[0x90, 60, 100]);
        if let Some(index) = external {
            // Give the separate note input time to arrive before advancing its clock.
            thread::sleep(Duration::from_millis(20));
            let timing = if index == source_index {
                &source
            } else {
                &clock
            };
            send(timing, &[0xfa]);
            send(timing, &[0xf8]);
        }
        receive_note(&receiver, 0x90);
        assert_eq!(host.finish(true), (true, String::new()));
        receive_note(&receiver, 0x80);
        println!("native arpeg note delivery and quit cleanup passed: clock={external:?}");
    }

    let mut host = Host::start(source_index, destination_index, None, true);
    send(&source, &[0x90, 60, 100]);
    let (success, stderr) = host.finish(false);
    assert!(!success);
    assert!(stderr.contains("MIDI transport input lost"), "{stderr}");
    assert!(
        receiver.try_recv().is_err(),
        "dropped input produced output"
    );
    println!("native arpeg transport-loss termination passed");
    source.close().unwrap();
    clock.close().unwrap();
    assert_eq!(destination.dropped_packets(), 0);
    assert!(!destination.callback_panicked());
    destination.close().unwrap();
}
