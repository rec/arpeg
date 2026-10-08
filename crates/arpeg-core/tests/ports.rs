use arpeg_core::ports::{OutputPort, PerformancePorts};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn capture_publication_reserves_its_whole_step_and_reports_exhaustion() {
    let mut ports = PerformancePorts::new("4/5".parse().unwrap(), 1.into());
    for index in 0..2047 {
        assert!(ports.begin_step(index.into(), index, 0, false));
        ports.outcome(false);
    }
    assert!(!ports.begin_step(2047.into(), 2047, 1, true));
    let batch = ports.take_events();
    assert!(batch.exhausted);
    assert_eq!(batch.events.len(), 4094);
    assert!(
        batch
            .events
            .iter()
            .all(|e| e.port != OutputPort::CaptureReady)
    );
    assert!(ports.begin_step(2048.into(), 2048, 1, false));
    ports.outcome(false);
    assert_eq!(
        ports
            .take_events()
            .events
            .iter()
            .map(|e| e.port)
            .collect::<Vec<_>>(),
        [OutputPort::Step, OutputPort::Rest]
    );
}
