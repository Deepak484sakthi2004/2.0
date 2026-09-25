// verify: debug ok
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

/// Option 1: an open set. Any Fee type, one heap allocation, dynamic dispatch.
fn fee_boxed(country: &str) -> Box<dyn Fee> {
    if country == "IE" { Box::new(Percent { bps: 29 }) } else { Box::new(Flat { cents: 30 }) }
}

/// Option 2: a closed set. No allocation; a match instead of a vtable.
enum AnyFee {
    Percent(Percent),
    Flat(Flat),
}

impl Fee for AnyFee {
    fn fee(&self, amount: i64) -> i64 {
        match self {
            AnyFee::Percent(p) => p.fee(amount),
            AnyFee::Flat(f) => f.fee(amount),
        }
    }
}

fn fee_enum(country: &str) -> impl Fee {
    if country == "IE" { AnyFee::Percent(Percent { bps: 29 }) } else { AnyFee::Flat(Flat { cents: 30 }) }
}

fn main() {
    for country in ["IE", "DE"] {
        println!("{country}: boxed {} / enum {}", fee_boxed(country).fee(10_000), fee_enum(country).fee(10_000));
    }
}
