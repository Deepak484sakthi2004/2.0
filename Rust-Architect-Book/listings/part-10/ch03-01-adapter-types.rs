// verify: debug ok
// Each adapter wraps the previous iterator in a new struct. The pipeline is one nested value on the stack.
use std::any::type_name_of_val;
use std::mem::size_of_val;

fn show<T>(label: &str, it: &T) {
    let name = type_name_of_val(it).replace("core::iter::adapters::", "").replace("core::slice::iter::", "");
    println!("{label:<12} {:>2} B  {name}", size_of_val(it));
}

fn main() {
    let amounts: Vec<u64> = vec![120, 5, 980, 42, 3_000];
    let fee_bps = 290_u64;

    let s0 = amounts.iter();
    show("iter()", &s0);
    let s1 = s0.filter(|a| **a >= 100); // closure captures nothing: 0 bytes
    show(".filter()", &s1);
    let s2 = s1.map(|a| a * fee_bps / 10_000); // closure captures &fee_bps: 8 bytes
    show(".map()", &s2);
    let s3 = s2.take(2); // adds a counter
    show(".take(2)", &s3);
    let s4 = s3.enumerate(); // adds another counter
    show(".enumerate()", &s4);

    let fees: Vec<(usize, u64)> = s4.collect();
    println!("result: {fees:?}");
}
