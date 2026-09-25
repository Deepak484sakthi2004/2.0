// verify: debug+nightly ok
// verify: debug error:E0554
// [VERSION] `gen` blocks: nightly-only (feature `gen_blocks`); `gen` is a reserved keyword in edition 2024.
// On stable this file is rejected (E0554: #![feature] may not be used on the stable release channel).
#![feature(gen_blocks)]

fn backoff(start: u64, cap: u64) -> impl Iterator<Item = u64> {
    gen move {
        let mut d = start;
        loop {
            yield d; // the block is compiled to a state machine that implements Iterator
            d = (d * 2).min(cap);
        }
    }
}

fn main() {
    let delays: Vec<u64> = backoff(100, 2_000).take(6).collect();
    println!("backoff ms: {delays:?}");
}
