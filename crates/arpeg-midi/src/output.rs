//! Optional byte budget and ownership of successfully sent MIDI notes.

use std::collections::BTreeMap;

pub struct MidiDelivery {
    bytes_per_second: Option<u64>,
    burst_bytes: usize,
    credit: u128,
    updated_ns: Option<u128>,
    failed: bool,
    owned: BTreeMap<u16, usize>,
}

impl MidiDelivery {
    pub fn new(bytes_per_second: Option<u64>, burst_bytes: usize) -> Result<Self, &'static str> {
        if bytes_per_second == Some(0) || burst_bytes == 0 {
            return Err("output rate and burst allowance must be positive");
        }
        Ok(Self {
            bytes_per_second,
            burst_bytes,
            credit: 0,
            updated_ns: None,
            failed: false,
            owned: BTreeMap::new(),
        })
    }

    pub fn send(
        &mut self,
        messages: &[Vec<u8>],
        at_ns: u128,
        mut write: impl FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        if self.failed {
            return Err(OVERLOAD.into());
        }
        if let Some(rate) = self.bytes_per_second.filter(|_| !messages.is_empty()) {
            let ceiling = self.burst_bytes as u128 * 1_000_000_000;
            let available = self.updated_ns.map_or(ceiling, |previous| {
                self.credit
                    .saturating_add((at_ns - previous).saturating_mul(u128::from(rate)))
                    .min(ceiling)
            });
            let required = messages
                .iter()
                .map(|m| m.len() as u128 * 1_000_000_000)
                .sum::<u128>();
            if required > available {
                self.failed = true;
                return Err(OVERLOAD.into());
            }
            self.credit = available - required;
            self.updated_ns = Some(at_ns);
        }
        for message in messages {
            write(message)?;
            self.record(message);
        }
        Ok(())
    }

    /// Cleanup bypasses the budget and releases only notes accepted by the port.
    pub fn cleanup(
        &mut self,
        messages: &[Vec<u8>],
        mut write: impl FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        for message in messages {
            if let [status, key, velocity] = message.as_slice() {
                let note = u16::from(status & 15) * 128 + u16::from(*key);
                if (status & 0xf0 == 0x80 || status & 0xf0 == 0x90 && *velocity == 0)
                    && self.owned.contains_key(&note)
                {
                    write(message)?;
                    self.record(message);
                }
            }
        }
        while let Some((&note, _)) = self.owned.first_key_value() {
            let message = [0x80 | (note / 128) as u8, (note % 128) as u8, 0];
            write(&message)?;
            self.record(&message);
        }
        Ok(())
    }

    fn record(&mut self, message: &[u8]) {
        let [status, key, velocity] = message else {
            return;
        };
        let note = u16::from(status & 15) * 128 + u16::from(*key);
        if status & 0xf0 == 0x90 && *velocity > 0 {
            *self.owned.entry(note).or_default() += 1;
        } else if status & 0xf0 == 0x80 || status & 0xf0 == 0x90 && *velocity == 0 {
            match self.owned.get_mut(&note) {
                Some(count) if *count > 1 => *count -= 1,
                _ => {
                    self.owned.remove(&note);
                }
            }
        }
    }
}

const OVERLOAD: &str = "MIDI output bandwidth exceeded; play session ended";

#[cfg(test)]
mod tests {
    use super::MidiDelivery;

    #[test]
    fn disabled_budget_preserves_dense_controller_samples() {
        let mut delivery = MidiDelivery::new(None, 1).unwrap();
        let messages: Vec<_> = (0..256).map(|n| vec![0xb0, 2, (n % 128) as u8]).collect();
        let mut sent = Vec::new();
        delivery
            .send(&messages, 0, |m| {
                sent.push(m.to_vec());
                Ok(())
            })
            .unwrap();
        assert_eq!(sent, messages);
    }

    #[test]
    fn byte_credit_refills_exactly_with_elapsed_time() {
        for (at_ns, accepted) in [(999_999_999, false), (1_000_000_000, true)] {
            let mut delivery = MidiDelivery::new(Some(3), 3).unwrap();
            let messages = [vec![0xb0, 2, 64]];
            let mut sent = Vec::new();
            delivery
                .send(&messages, 0, |m| {
                    sent.push(m.to_vec());
                    Ok(())
                })
                .unwrap();
            let result = delivery.send(&messages, at_ns, |m| {
                sent.push(m.to_vec());
                Ok(())
            });
            assert_eq!(result.is_ok(), accepted);
            assert_eq!(sent.len(), if accepted { 2 } else { 1 });
        }
    }

    #[test]
    fn long_idle_cannot_exceed_burst_or_restart_after_failure() {
        let mut delivery = MidiDelivery::new(Some(3), 3).unwrap();
        delivery.send(&[vec![0xf8]], 0, |_| Ok(())).unwrap();
        assert!(
            delivery
                .send(&vec![vec![0xf8]; 4], 100_000_000_000, |_| panic!(
                    "refused batch sent"
                ))
                .is_err()
        );
        assert!(
            delivery
                .send(&[vec![0xf8]], 200_000_000_000, |_| panic!(
                    "failed output restarted"
                ))
                .is_err()
        );
    }

    #[test]
    fn refused_handoff_preserves_entry_group_and_releases_only_the_sent_note() {
        let mut delivery = MidiDelivery::new(Some(3), 6).unwrap();
        let mut sent = Vec::new();
        delivery
            .send(&[vec![0x90, 60, 100]], 0, |m| {
                sent.push(m.to_vec());
                Ok(())
            })
            .unwrap();
        assert!(
            delivery
                .send(
                    &[vec![0x80, 60, 0], vec![0xb0, 2, 80], vec![0x90, 64, 90]],
                    0,
                    |_| panic!("partial handoff sent")
                )
                .is_err()
        );
        delivery
            .cleanup(&[vec![0x80, 64, 0]], |m| {
                sent.push(m.to_vec());
                Ok(())
            })
            .unwrap();
        assert_eq!(sent, [vec![0x90, 60, 100], vec![0x80, 60, 0]]);
    }

    #[test]
    fn cleanup_preserves_release_velocity_and_counts_velocity_zero_releases() {
        let mut delivery = MidiDelivery::new(None, 64).unwrap();
        let mut sent = Vec::new();
        delivery
            .send(
                &[vec![0x92, 60, 100], vec![0x92, 60, 100], vec![0x92, 60, 0]],
                0,
                |m| {
                    sent.push(m.to_vec());
                    Ok(())
                },
            )
            .unwrap();
        delivery
            .cleanup(&[vec![0x82, 60, 20]], |m| {
                sent.push(m.to_vec());
                Ok(())
            })
            .unwrap();
        delivery
            .cleanup(&[], |_| panic!("duplicate cleanup"))
            .unwrap();
        assert_eq!(
            sent,
            [
                vec![0x92, 60, 100],
                vec![0x92, 60, 100],
                vec![0x92, 60, 0],
                vec![0x82, 60, 20]
            ]
        );
    }

    #[test]
    fn failed_send_is_not_recorded_as_owned() {
        let mut delivery = MidiDelivery::new(None, 64).unwrap();
        let mut sent = Vec::new();
        delivery
            .send(&[vec![0x90, 60, 100]], 0, |m| {
                sent.push(m.to_vec());
                Ok(())
            })
            .unwrap();
        assert!(
            delivery
                .send(&[vec![0x90, 64, 90]], 0, |_| Err("disconnected".into()))
                .is_err()
        );
        delivery
            .cleanup(&[], |m| {
                sent.push(m.to_vec());
                Ok(())
            })
            .unwrap();
        assert_eq!(sent, [vec![0x90, 60, 100], vec![0x80, 60, 0]]);
    }
}
