// verify: debug ok
// verify: release ok
// What a stack sampler can attribute to: the frames that exist at run time. In release, small functions are
// inlined into their callers, so they don't have frames of their own.
use std::backtrace::Backtrace;
use std::hint::black_box;

fn validate_amount(cents: i64) -> String {
    let bt = Backtrace::force_capture().to_string();
    black_box(cents);
    bt
}

fn build_refund(cents: i64) -> String {
    validate_amount(cents)
}

fn handle_refund(cents: i64) -> String {
    build_refund(cents)
}

fn main() {
    let bt = handle_refund(black_box(2_500));
    let ours: Vec<&str> = bt
        .lines()
        .map(str::trim)
        .filter(|l| l.contains("playground::"))
        .map(|l| l.split_once(": ").map_or(l, |(_, name)| name))
        .collect();
    println!("{} frames from this crate:", ours.len());
    for f in ours {
        println!("  {f}");
    }
}
