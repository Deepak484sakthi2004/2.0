// verify: debug ok
/// A GENERIC trait: one type can implement it many times, once per `T`.
trait Convert<T> {
    fn convert(&self) -> T;
}

struct Cents(i64);

impl Convert<f64> for Cents {
    fn convert(&self) -> f64 {
        self.0 as f64 / 100.0
    }
}

impl Convert<String> for Cents {
    fn convert(&self) -> String {
        format!("{}.{:02}", self.0 / 100, self.0 % 100)
    }
}

// The std pattern: From is generic (many sources), Into comes from a BLANKET impl over From.
struct Basis(u32);

impl From<u32> for Basis {
    fn from(bps: u32) -> Self {
        Basis(bps)
    }
}

impl From<f64> for Basis {
    fn from(percent: f64) -> Self {
        Basis((percent * 100.0).round() as u32)
    }
}

fn main() {
    let price = Cents(12_550);
    let as_float: f64 = price.convert(); // the annotation selects the impl
    let as_text = <Cents as Convert<String>>::convert(&price); // or fully qualified syntax
    println!("{as_float} / {as_text}");

    let a: Basis = 250u32.into(); // Into<Basis> for u32 exists because From<u32> for Basis does
    let b = Basis::from(1.75);
    println!("{} bps, {} bps", a.0, b.0);
}
