// verify: debug error:E0599
use std::collections::HashMap;

fn main() {
    let mut fx_rates: HashMap<f64, &str> = HashMap::new();
    fx_rates.insert(1.0842, "EURUSD");
    println!("{fx_rates:?}");
}
