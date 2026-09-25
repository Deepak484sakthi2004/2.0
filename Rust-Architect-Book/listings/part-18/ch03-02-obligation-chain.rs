// verify: debug error:E0277
// An E0277 is a failed proof. The notes print the chain of obligations the solver followed:
// record needs `Vec<Cents>: Audit` -> the impl needs `Cents: Display` -> no impl.
use std::fmt::Display;

trait Audit {
    fn audit(&self) -> String;
}

impl<T: Display> Audit for Vec<T> {
    fn audit(&self) -> String {
        self.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")
    }
}

fn record<A: Audit>(a: &A) {
    println!("{}", a.audit());
}

struct Cents(i64); // no Display

fn main() {
    record(&vec![Cents(100), Cents(250)]);
}
