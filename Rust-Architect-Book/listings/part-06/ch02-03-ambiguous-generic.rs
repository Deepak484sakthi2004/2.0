// verify: debug error:E0282
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

fn main() {
    let price = Cents(12_550);
    let shown = price.convert(); // which Convert<T>?
    println!("{}", shown.len());
}
