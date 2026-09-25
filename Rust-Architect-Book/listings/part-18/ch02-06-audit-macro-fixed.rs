// verify: debug ok
// The fix: name the function by an absolute path. `$crate` always refers to the crate that defines
// the macro, whichever module or crate calls it.
mod audit {
    pub fn mask(pan: &str) -> String {
        let last4 = &pan[pan.len() - 4..];
        format!("****{last4}")
    }
}

macro_rules! audit {
    ($event:expr, $pan:expr) => {
        println!("audit: {} card={}", $event, $crate::audit::mask($pan))
    };
}

mod checkout {
    pub fn pay(pan: &str) {
        audit!("charge", pan);
    }
}

mod refunds {
    #[allow(dead_code)]
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
