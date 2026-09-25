// verify: debug ok
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
enum ChargeError {
    Timeout,
}

#[derive(Default)]
struct Payments {
    charges: u32,
    keys: HashMap<String, u32>,
}

impl Payments {
    /// Idempotent by key: a repeated key returns the original charge number.
    fn charge(&mut self, key: &str) -> Result<u32, ChargeError> {
        if let Some(&n) = self.keys.get(key) {
            return Ok(n);
        }
        self.charges += 1;
        self.keys.insert(key.to_string(), self.charges);
        Ok(self.charges)
    }
}

fn main() {
    let mut payments = Payments::default();
    let mut next = 0;
    let mut new_key = || {
        next += 1;
        format!("idem-{next}")
    };
    // The gateway's retry loop. The first response is lost in the network.
    for attempt in 0..3 {
        let key = new_key();
        let result = payments.charge(&key).and_then(|n| if attempt == 0 { Err(ChargeError::Timeout) } else { Ok(n) });
        println!("attempt {attempt} key={key} -> {result:?}");
        if result.is_ok() {
            break;
        }
    }
    println!("charges={}", payments.charges);
}
