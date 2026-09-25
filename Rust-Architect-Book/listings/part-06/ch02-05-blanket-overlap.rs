// verify: debug error:E0119
use std::fmt;

// --- the logging library (v1.4 added the blanket impl for convenience) ---
trait LogLine {
    fn log_line(&self) -> String;
}

impl<T: fmt::Display> LogLine for T {
    fn log_line(&self) -> String {
        format!("[INFO] {self}")
    }
}

// --- the payments code (written against v1.3) ---
struct Money {
    cents: i64,
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{:02} EUR", self.cents / 100, self.cents % 100)
    }
}

// A hand-written impl that redacts amounts in logs. Money is Display, so the blanket impl covers it too.
impl LogLine for Money {
    fn log_line(&self) -> String {
        "[INFO] <amount redacted>".to_string()
    }
}

fn main() {
    println!("{}", Money { cents: 12_550 }.log_line());
}
