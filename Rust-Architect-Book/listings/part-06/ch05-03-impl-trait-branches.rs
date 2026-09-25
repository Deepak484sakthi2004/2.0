// verify: debug error:E0308
trait Fee {
    fn fee(&self, amount: i64) -> i64;
}

struct Percent {
    bps: i64,
}
struct Flat {
    cents: i64,
}

impl Fee for Percent {
    fn fee(&self, amount: i64) -> i64 {
        amount * self.bps / 10_000
    }
}
impl Fee for Flat {
    fn fee(&self, _amount: i64) -> i64 {
        self.cents
    }
}

/// "Return some Fee": but `impl Trait` means ONE concrete type chosen by the function.
fn fee_for(country: &str) -> impl Fee {
    if country == "IE" { Percent { bps: 29 } } else { Flat { cents: 30 } }
}

fn main() {
    println!("{}", fee_for("IE").fee(10_000));
}
