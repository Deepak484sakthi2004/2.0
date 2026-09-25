// verify: debug ok
use ordered_float::OrderedFloat;
use std::collections::{BTreeMap, HashMap};

fn main() {
    // Option 1: a wrapper that defines Eq/Ord/Hash for floats (NaN == NaN, NaN sorts last, -0.0 == 0.0).
    let mut by_rate: HashMap<OrderedFloat<f64>, &str> = HashMap::new();
    by_rate.insert(OrderedFloat(1.0842), "EURUSD");
    by_rate.insert(OrderedFloat(f64::NAN), "broken feed");
    println!("lookup 1.0842 -> {:?}", by_rate.get(&OrderedFloat(1.0842)));
    println!("lookup NaN    -> {:?}", by_rate.get(&OrderedFloat(f64::NAN)));

    // Option 2 (usually better for money): don't key by float at all. Integer minor units.
    let mut levels: BTreeMap<i64, u64> = BTreeMap::new(); // price in 1/10_000 units -> quantity
    levels.insert(10_842, 500);
    levels.insert(10_841, 200);
    println!("best level: {:?}", levels.last_key_value());

    // Sorting floats: total_cmp gives a total order without a wrapper type.
    let mut xs = vec![2.5, -0.0, f64::NAN, 0.0, -1.0];
    xs.sort_by(f64::total_cmp);
    println!("sorted with total_cmp: {xs:?}");
}
