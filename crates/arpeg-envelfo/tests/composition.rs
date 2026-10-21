use arpeg_core::{
    clock::ClockMode,
    ports::{InputPort, OutputPort, PortEvent},
};
use arpeg_envelfo::EnveLFOBridge;
use arpeg_midi::{
    parse_profile,
    player::{InputSource, MidiPlayer},
};
use envelfo_core::{
    envelope::{Envelope, Marker, Retrigger, Segment, Start},
    lfo::{LFO, Shape},
    patch::{
        Child, ClockMapping, Connection, ConnectionAction, Node, Output, Patch, Port, Seconds,
    },
    signal::Operator,
    time::Time,
};

#[test]
fn composition_matches_python_wire_trace() {
    let trace: toml::Value = toml::from_str(include_str!(
        "../../../conformance/envelfo-composition.toml"
    ))
    .unwrap();
    for poll_us in [None, Some(1000)] {
        let mut player = MidiPlayer::new(
            parse_profile(trace["profile"].as_str().unwrap(), None).unwrap(),
            ClockMode::Internal,
            120,
            500_000,
        )
        .unwrap();
        let mut bridge = bridge(false);
        let mut previous = 0;
        for action in trace["actions"].as_array().unwrap() {
            let tick = action["at_us"].as_integer().unwrap();
            let mut output = vec![];
            if let Some(poll_us) = poll_us {
                let mut poll = previous + poll_us;
                while poll < tick {
                    output.extend(player.advance(poll).unwrap());
                    let batch = player.take_events();
                    assert!(!batch.exhausted);
                    assert!(!batch.events.iter().any(|e| e.port == OutputPort::Hit));
                    poll += poll_us;
                }
            }
            if let Some(data) = action.get("data") {
                for data in data.as_array().unwrap() {
                    let bytes: Vec<u8> = data
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_integer().unwrap().try_into().unwrap())
                        .collect();
                    output.extend(player.accept(tick, &bytes, InputSource::Both).unwrap());
                }
            }
            if action.get("stop").and_then(toml::Value::as_bool) == Some(true) {
                output.extend(player.stop(tick).unwrap());
            }
            player
                .schedule(
                    tick,
                    action
                        .get("tempo")
                        .map(|t| t.as_integer().unwrap().try_into().unwrap()),
                )
                .unwrap();
            let batch = player.take_events();
            assert!(!batch.exhausted);
            let at = Time::new(tick, 1_000_000).unwrap();
            for event in batch.events {
                bridge.receive(at, &event, &Seconds).unwrap();
            }
            let result = bridge
                .observe(
                    at,
                    at.checked_add(Time::new(1, 1000).unwrap()).unwrap(),
                    &Seconds,
                )
                .unwrap();
            for name in ["breath", "accent"] {
                assert!((result.values[name] - action[name].as_float().unwrap()).abs() < 1e-12);
            }
            output.extend(player.realize(&result.controls).unwrap());
            let expected: Vec<Vec<u8>> = action["output"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| {
                    a.as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_integer().unwrap().try_into().unwrap())
                        .collect()
                })
                .collect();
            assert_eq!(output, expected, "{action}");
            // The portable Rust continuation contract is Clone, not serialized JSON.
            bridge = bridge.clone();
            previous = tick;
        }
    }
}

#[test]
fn pending_delivery_survives_clone_without_replaying_a_hit() {
    let mut bridge = bridge(true);
    bridge.receive(Time::ZERO, &hit(0), &Seconds).unwrap();
    bridge
        .observe(
            Time::new(1, 4).unwrap(),
            Time::new(3, 10).unwrap(),
            &Seconds,
        )
        .unwrap();
    assert_eq!(bridge.state.pending_count(), 1);
    let saved = bridge.clone();
    let mut restored = saved.clone();
    let expected = bridge
        .observe(
            Time::new(3, 10).unwrap(),
            Time::new(2, 5).unwrap(),
            &Seconds,
        )
        .unwrap();
    let actual = restored
        .observe(
            Time::new(3, 10).unwrap(),
            Time::new(2, 5).unwrap(),
            &Seconds,
        )
        .unwrap();
    assert_eq!(actual, expected);
    assert!((actual.values["captured"] - 0.6).abs() < 1e-12);
    assert_eq!(restored.state.pending_count(), 0);
    assert_eq!(saved.state.pending_count(), 1);
}

#[test]
fn same_time_hits_and_time_reversal_preserve_scheduler_contracts() {
    let mut bridge = bridge(false);
    bridge.receive(Time::ZERO, &hit(0), &Seconds).unwrap();
    bridge.receive(Time::ZERO, &hit(1), &Seconds).unwrap();
    let result = bridge
        .observe(Time::new(1, 8).unwrap(), Time::ONE, &Seconds)
        .unwrap();
    assert_eq!(result.values["accent"], 0.5);
    let saved = bridge.clone();
    assert!(bridge.receive(Time::ZERO, &hit(2), &Seconds).is_err());
    assert_eq!(bridge, saved);
}

#[test]
fn invalid_expression_does_not_commit_an_observation() {
    let mut definition = definition(false);
    if let Node::LFO { body, .. } = &mut definition.children[0].node {
        body.center = 0.0;
        body.depth = 1.0;
    }
    let state = definition
        .initial(Time::ZERO, "patch", 0, None, &Seconds)
        .unwrap();
    let mut bridge = EnveLFOBridge::new(
        definition,
        state,
        "attack".into(),
        vec![(InputPort::Breath, "breath".into())],
    )
    .unwrap();
    let saved = bridge.clone();
    assert!(bridge.observe(Time::ZERO, Time::ONE, &Seconds).is_err());
    assert_eq!(bridge, saved);
}

#[test]
fn host_tempo_maps_each_child_without_a_second_clock() {
    struct HostClock;
    impl ClockMapping for HostClock {
        fn local_time(&self, child: &str, seconds: Time) -> Result<Time, envelfo_core::Error> {
            if child == "attack" {
                seconds.checked_mul(Time::from_integer(2))
            } else {
                Ok(seconds)
            }
        }
        fn host_time(&self, child: &str, local: Time) -> Result<Option<Time>, envelfo_core::Error> {
            Ok(Some(if child == "attack" {
                local.checked_div(Time::from_integer(2))?
            } else {
                local
            }))
        }
    }
    let definition = definition(false);
    let state = definition
        .initial(Time::ZERO, "patch", 0, None, &HostClock)
        .unwrap();
    let mut bridge = EnveLFOBridge::new(
        definition,
        state,
        "attack".into(),
        vec![
            (InputPort::Breath, "breath".into()),
            (InputPort::Pressure, "accent".into()),
        ],
    )
    .unwrap();
    bridge.receive(Time::ZERO, &hit(0), &HostClock).unwrap();
    let result = bridge
        .observe(
            Time::new(1, 16).unwrap(),
            Time::new(1, 8).unwrap(),
            &HostClock,
        )
        .unwrap();
    assert_eq!(result.values["breath"], 0.125);
    assert_eq!(result.values["accent"], 0.5);
}

fn hit(index: i64) -> PortEvent {
    PortEvent {
        channel: 1,
        at: 0.into(),
        port: OutputPort::Hit,
        index,
        revision: 1,
    }
}

fn bridge(retained: bool) -> EnveLFOBridge {
    let definition = definition(retained);
    let state = definition
        .initial(Time::ZERO, "patch", 0, None, &Seconds)
        .unwrap();
    EnveLFOBridge::new(
        definition,
        state,
        "attack".into(),
        vec![
            (InputPort::Breath, "breath".into()),
            (InputPort::Pressure, "accent".into()),
        ],
    )
    .unwrap()
}

fn definition(retained: bool) -> Patch {
    let mut wave = LFO::new(Time::ONE).unwrap();
    wave.shape = Shape::Triangle;
    wave.center = 0.5;
    wave.depth = 0.5;
    let mut attack = Envelope::new(
        0.0,
        vec![Segment {
            duration: Some(Time::new(1, 4).unwrap()),
            to: 1.0,
            curve: 0.0,
        }],
    )
    .unwrap();
    attack.start = Start::Event;
    attack.retrigger = Retrigger::Initial;
    let mut definition = Patch {
        children: vec![
            Child {
                name: "wave".into(),
                node: Node::LFO {
                    body: wave,
                    markers: if retained {
                        vec![Marker {
                            name: "tick".into(),
                            position: Time::new(1, 4).unwrap(),
                        }]
                    } else {
                        vec![]
                    },
                },
            },
            Child {
                name: "attack".into(),
                node: Node::Envelope(attack),
            },
        ],
        outputs: vec![
            Output {
                name: "breath".into(),
                source: "wave".into(),
            },
            Output {
                name: "accent".into(),
                source: "attack".into(),
            },
        ],
        event_outputs: vec![],
        connections: vec![],
    };
    if retained {
        definition.children.push(Child {
            name: "held".into(),
            node: Node::Signal(Operator::Latch("wave".into())),
        });
        definition.outputs.push(Output {
            name: "captured".into(),
            source: "held".into(),
        });
        definition.connections.push(Connection {
            source: Port {
                child: "wave".into(),
                port: "tick".into(),
            },
            target: "held".into(),
            action: ConnectionAction::Capture,
            every: 1,
            offset: 0,
            probability: 1.0,
            delay: Time::new(1, 20).unwrap(),
        });
    }
    definition
}
