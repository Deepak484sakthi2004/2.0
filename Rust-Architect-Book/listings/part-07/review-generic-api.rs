// verify: debug test
//! Meridian's event publisher after the Part VII review:
//! generic at the edge where types differ, `dyn` and plain functions where they don't.

/// How an event turns itself into bytes. Implemented per event type (hot, inlinable).
pub trait Encode {
    fn encode(&self, out: &mut Vec<u8>);
}

/// Where bytes go. Implemented by the Kafka client, a file sink, or a test double.
pub trait Transport {
    fn send(&mut self, topic: &str, bytes: &[u8]) -> Result<(), String>;
}

pub struct Publisher {
    transport: Box<dyn Transport>, // one copy of all sending code, whatever the transport is
    buf: Vec<u8>,                  // reused across publishes: no allocation per event once warm
    pub sent: u64,
    pub failed: u64,
}

impl Publisher {
    pub fn new(transport: impl Transport + 'static) -> Self {
        Publisher { transport: Box::new(transport), buf: Vec::with_capacity(512), sent: 0, failed: 0 }
    }

    /// The only generic method: a thin shim that encodes, then hands off to shared code.
    pub fn publish<E: Encode + ?Sized>(&mut self, topic: &str, event: &E) -> Result<(), String> {
        self.buf.clear();
        event.encode(&mut self.buf);
        self.send_encoded(topic)
    }

    /// Non-generic: retries, metrics, and error mapping are compiled once.
    fn send_encoded(&mut self, topic: &str) -> Result<(), String> {
        let mut last = String::new();
        for _attempt in 0..3 {
            match self.transport.send(topic, &self.buf) {
                Ok(()) => {
                    self.sent += 1;
                    return Ok(());
                }
                Err(e) => last = e,
            }
        }
        self.failed += 1;
        Err(format!("{topic}: gave up after 3 attempts: {last}"))
    }
}

pub struct PaymentCaptured {
    pub payment_id: u64,
    pub amount_cents: i64,
}
impl Encode for PaymentCaptured {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.payment_id.to_le_bytes());
        out.extend_from_slice(&self.amount_cents.to_le_bytes());
    }
}
impl Encode for str {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(self.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Test double: records what was sent; fails the first `flaky` sends.
    struct Memory {
        log: Rc<RefCell<Vec<(String, Vec<u8>)>>>,
        flaky: u32,
    }
    impl Transport for Memory {
        fn send(&mut self, topic: &str, bytes: &[u8]) -> Result<(), String> {
            if self.flaky > 0 {
                self.flaky -= 1;
                return Err("broker unavailable".into());
            }
            self.log.borrow_mut().push((topic.to_string(), bytes.to_vec()));
            Ok(())
        }
    }

    #[test]
    fn publishes_two_event_types_through_one_transport() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut p = Publisher::new(Memory { log: Rc::clone(&log), flaky: 0 });
        p.publish("payments", &PaymentCaptured { payment_id: 7, amount_cents: 1250 }).unwrap();
        p.publish("audit", "captured 7").unwrap();
        assert_eq!(p.sent, 2);
        assert_eq!(log.borrow()[0].1.len(), 16);
        assert_eq!(log.borrow()[1].1, b"captured 7");
    }

    #[test]
    fn retries_then_gives_up() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut p = Publisher::new(Memory { log: Rc::clone(&log), flaky: 2 });
        assert!(p.publish("audit", "ok on third try").is_ok());
        let mut q = Publisher::new(Memory { log, flaky: 5 });
        let err = q.publish("audit", "never").unwrap_err();
        assert!(err.contains("gave up after 3 attempts"), "{err}");
        assert_eq!((q.sent, q.failed), (0, 1));
    }
}
