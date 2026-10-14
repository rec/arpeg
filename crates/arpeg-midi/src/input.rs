//! Bounded host input, with a failure signal independent of queue capacity.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy)]
pub struct InputLimits {
    pub packets: usize,
    pub bytes: usize,
    pub max_message_bytes: usize,
}

impl Default for InputLimits {
    fn default() -> Self {
        Self {
            packets: 4096,
            bytes: 1048576,
            max_message_bytes: 65536,
        }
    }
}

#[derive(Default)]
pub struct InputFailure(Mutex<Option<String>>);

impl InputFailure {
    pub fn fail(&self, reason: String) {
        self.0
            .lock()
            .expect("input failure lock")
            .get_or_insert(reason);
    }

    pub fn check(&self) -> Result<(), String> {
        match self.0.lock().expect("input failure lock").as_ref() {
            Some(reason) => Err(reason.clone()),
            None => Ok(()),
        }
    }
}

pub struct Inbox<T> {
    limits: InputLimits,
    label: &'static str,
    failure: Arc<InputFailure>,
    state: Mutex<Queued<T>>,
}

struct Queued<T> {
    items: VecDeque<(T, usize)>,
    bytes: usize,
}

impl<T> Inbox<T> {
    pub fn new(limits: InputLimits, label: &'static str, failure: Arc<InputFailure>) -> Self {
        Self {
            limits,
            label,
            failure,
            state: Mutex::new(Queued {
                items: VecDeque::new(),
                bytes: 0,
            }),
        }
    }

    /// Allocate retained payload only after reserving capacity; never wait for space.
    pub fn put(&self, size: usize, value: impl FnOnce() -> T) {
        let mut failure = self.failure.0.lock().expect("input failure lock");
        if failure.is_some() {
            return;
        }
        let mut state = self.state.lock().expect("input queue lock");
        if size > self.limits.max_message_bytes
            || state.items.len() >= self.limits.packets
            || size > self.limits.bytes.saturating_sub(state.bytes)
        {
            *failure = Some(format!("{} overflow; play session ended", self.label));
            return;
        }
        state.items.push_back((value(), size));
        state.bytes += size;
    }

    pub fn pop(&self) -> Result<Option<T>, String> {
        let failure = self.failure.0.lock().expect("input failure lock");
        if let Some(reason) = failure.as_ref() {
            return Err(reason.clone());
        }
        let mut state = self.state.lock().expect("input queue lock");
        Ok(state.items.pop_front().map(|(value, size)| {
            state.bytes -= size;
            value
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{Inbox, InputFailure, InputLimits};
    use std::{sync::Arc, thread};

    #[test]
    fn draining_restores_count_and_bytes_in_fifo_order() {
        let inbox = Inbox::new(
            InputLimits {
                packets: 2,
                bytes: 5,
                max_message_bytes: 3,
            },
            "MIDI input",
            Arc::new(InputFailure::default()),
        );
        inbox.put(3, || 1);
        inbox.put(2, || 2);
        assert_eq!(inbox.pop().unwrap(), Some(1));
        inbox.put(3, || 3);
        assert_eq!(inbox.pop().unwrap(), Some(2));
        assert_eq!(inbox.pop().unwrap(), Some(3));
        assert_eq!(inbox.pop().unwrap(), None);
    }

    #[test]
    fn each_capacity_limit_latches_failure_before_allocating_payload() {
        for (limits, sizes) in [
            (
                InputLimits {
                    packets: 1,
                    bytes: 10,
                    max_message_bytes: 10,
                },
                vec![1, 1],
            ),
            (
                InputLimits {
                    packets: 10,
                    bytes: 3,
                    max_message_bytes: 10,
                },
                vec![2, 2],
            ),
            (
                InputLimits {
                    packets: 10,
                    bytes: 10,
                    max_message_bytes: 3,
                },
                vec![4],
            ),
        ] {
            let failure = Arc::new(InputFailure::default());
            let inbox = Inbox::new(limits, "MIDI input", Arc::clone(&failure));
            for (i, size) in sizes.iter().enumerate() {
                inbox.put(*size, || {
                    assert!(i + 1 < sizes.len(), "overflow payload allocated");
                    1
                });
            }
            assert!(inbox.pop().unwrap_err().contains("MIDI input overflow"));
            inbox.put(1, || panic!("allocation after failure"));
            assert!(failure.check().is_err());
        }
    }

    #[test]
    fn concurrent_producers_share_capacity_and_notify_commands() {
        let failure = Arc::new(InputFailure::default());
        let inbox = Arc::new(Inbox::new(
            InputLimits {
                packets: 128,
                bytes: 128,
                max_message_bytes: 1,
            },
            "MIDI input",
            Arc::clone(&failure),
        ));
        let commands = Inbox::<String>::new(
            InputLimits::default(),
            "Console input",
            Arc::clone(&failure),
        );
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let inbox = Arc::clone(&inbox);
                thread::spawn(move || {
                    for _ in 0..65 {
                        inbox.put(1, || 1);
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(commands.pop().is_err());
    }
}
