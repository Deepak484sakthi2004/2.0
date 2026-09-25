// verify: debug ok
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
enum ChargeError {
    Timeout,
    IdempotencyConflict,
    InProgress,
}

#[derive(Debug, Clone, PartialEq)]
struct Charge {
    id: String,
    amount: i64,
}

enum Entry {
    InProgress { amount: i64 },
    Done { amount: i64, result: Result<Charge, ChargeError> },
}

/// Meridian payments. `charges` is the money that actually moved.
#[derive(Default)]
struct Payments {
    charges: Vec<Charge>,
    keys: HashMap<String, Entry>,
}

impl Payments {
    fn charge_card(&mut self, amount: i64) -> Result<Charge, ChargeError> {
        let c = Charge { id: format!("ch_{}", self.charges.len() + 1), amount };
        self.charges.push(c.clone());
        Ok(c)
    }

    /// No idempotency: every request that arrives is a new charge.
    fn charge_naive(&mut self, amount: i64) -> Result<Charge, ChargeError> {
        self.charge_card(amount)
    }

    /// With an idempotency key: a key identifies ONE logical operation, however many times it is delivered.
    fn charge(&mut self, key: &str, amount: i64) -> Result<Charge, ChargeError> {
        if let Some(entry) = self.keys.get(key) {
            return match entry {
                Entry::Done { amount: a, result } if *a == amount => result.clone(), // replay the original outcome
                Entry::InProgress { amount: a } if *a == amount => Err(ChargeError::InProgress), // concurrent duplicate
                _ => Err(ChargeError::IdempotencyConflict), // same key, different request: a client bug
            };
        }
        self.keys.insert(key.to_string(), Entry::InProgress { amount });
        let result = self.charge_card(amount); // in production: the processor call carries the key too
        self.keys.insert(key.to_string(), Entry::Done { amount, result: result.clone() });
        result
    }
}

/// The network between the gateway and payments: the request arrives, the response may not.
fn deliver<T>(response: Result<T, ChargeError>, lose_response: bool) -> Result<T, ChargeError> {
    if lose_response { Err(ChargeError::Timeout) } else { response }
}

fn main() {
    // The gateway sees a timeout and retries. Without a key, the customer pays twice.
    let mut naive = Payments::default();
    let first = deliver(naive.charge_naive(4_999), true);
    let retry = deliver(naive.charge_naive(4_999), false);
    println!("naive:      first={first:?} retry={retry:?} charges={}", naive.charges.len());

    let mut p = Payments::default();
    let key = "idem-2c9e";
    let first = deliver(p.charge(key, 4_999), true);
    let retry = deliver(p.charge(key, 4_999), false);
    let reuse = deliver(p.charge(key, 9_999), false);
    println!("idempotent: first={first:?}");
    println!("            retry={retry:?}");
    println!("            reuse={reuse:?}");
    println!("            charges={}", p.charges.len());
}
