// verify: debug ok
use std::any::Any;
use std::hint::black_box;
use std::panic;

fn index_out_of_bounds() -> u32 {
    let v = vec![1, 2, 3];
    v[black_box(7)]
}

fn unwrap_none() -> u32 {
    let port: Option<u32> = black_box(None);
    port.unwrap()
}

fn expect_err() -> u32 {
    black_box("80x").parse::<u32>().expect("PORT must be a number")
}

fn explicit_panic() -> u32 {
    panic!("ledger invariant violated: {} open transactions at shutdown", black_box(2))
}

fn overflow() -> u32 {
    let x: u8 = black_box(200);
    (x + 100) as u32 // debug build: overflow check panics
}

/// A panic payload is `Box<dyn Any + Send>`: usually a &'static str or a String, but not always.
fn describe(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        format!("&str   {s:?}")
    } else if let Some(s) = payload.downcast_ref::<String>() {
        format!("String {s:?}")
    } else {
        "<some other type>".to_string()
    }
}

fn main() {
    let cases: [(&str, fn() -> u32); 5] = [
        ("index", index_out_of_bounds),
        ("unwrap", unwrap_none),
        ("expect", expect_err),
        ("panic!", explicit_panic),
        ("overflow", overflow),
    ];
    for (name, f) in cases {
        match panic::catch_unwind(f) {
            Ok(v) => println!("{name:<9} returned {v}"),
            Err(payload) => println!("{name:<9} payload {}", describe(&*payload)),
        }
    }
}
