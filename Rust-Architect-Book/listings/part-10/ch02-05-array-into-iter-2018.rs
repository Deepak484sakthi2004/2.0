// verify: debug@2018 ok
// verify: debug@2021 ok
// Same source, two editions, two different item types (edition 2018 also warns: array_into_iter).
use std::any::type_name_of_val;

fn main() {
    let codes = [200_u16, 404, 503];
    let first = codes.into_iter().next().unwrap();
    println!("item type: {}", type_name_of_val(&first));
}
