// verify: debug ok
// macro_rules! hygiene covers local variables and labels, NOT items: the function name `mask` in
// the macro body is resolved where the macro is CALLED. In `refunds`, a local helper also called
// `mask` wins, and the audit line prints the card number in clear.
mod audit {
    /// Keep only the last four digits.
    pub fn mask(pan: &str) -> String {
        let last4 = &pan[pan.len() - 4..];
        format!("****{last4}")
    }
}

macro_rules! audit {
    ($event:expr, $pan:expr) => {
        println!("audit: {} card={}", $event, mask($pan)) // BUG: `mask` resolved at the call site
    };
}

mod checkout {
    use crate::audit::mask; // checkout works only because it happens to import the right `mask`
    pub fn pay(pan: &str) {
        audit!("charge", pan);
    }
}

mod refunds {
    /// A UI helper: formats the PAN for the agent's screen (which is access-controlled).
    fn mask(pan: &str) -> String {
        pan.to_string()
    }
    pub fn refund(pan: &str) {
        audit!("refund", pan);
    }
}

fn main() {
    checkout::pay("4111111111111111");
    refunds::refund("4111111111111111");
}
