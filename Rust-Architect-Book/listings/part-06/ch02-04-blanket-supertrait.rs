// verify: debug ok
use std::fmt::{Debug, Display};

/// A BLANKET impl: every type that is Display gets `log_line` for free.
trait LogLine {
    fn log_line(&self, level: &str) -> String;
}

impl<T: Display + ?Sized> LogLine for T {
    fn log_line(&self, level: &str) -> String {
        format!("[{level}] {self}")
    }
}

/// A SUPERTRAIT: anything Auditable must also be Debug, so default methods can use {:?}.
trait Auditable: Debug {
    fn actor(&self) -> &str;
    fn audit(&self) -> String {
        format!("{} did {:?}", self.actor(), self)
    }
}

#[derive(Debug)]
struct Refund {
    order: u64,
    cents: i64,
    by: String,
}

impl Auditable for Refund {
    fn actor(&self) -> &str {
        &self.by
    }
}

fn main() {
    println!("{}", 42.log_line("INFO"));
    println!("{}", "gateway started".log_line("INFO"));
    println!("{}", 3.5f64.log_line("WARN"));

    let r = Refund { order: 9001, cents: 2_500, by: "ada".into() };
    println!("{}", r.audit());
    println!("order {} for {} cents", r.order, r.cents);
}
