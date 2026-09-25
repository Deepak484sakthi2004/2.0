// verify: debug ok
// Iterators without a struct: from_fn (state in a closure) and successors (each item from the previous one).
use std::iter;

fn main() {
    // Exponential backoff schedule (Chapter 8.4): 100, 200, 400, ... capped at 2,000 ms, 6 attempts.
    let delays: Vec<u64> = iter::successors(Some(100_u64), |&d| Some((d * 2).min(2_000))).take(6).collect();
    println!("backoff ms: {delays:?}");

    // A tokenizer whose state (`rest`) is captured by a closure.
    let line = "SET  user:42   active";
    let mut rest = line;
    let tokens = iter::from_fn(move || {
        rest = rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        let end = rest.find(' ').unwrap_or(rest.len());
        let (tok, tail) = rest.split_at(end);
        rest = tail;
        Some(tok)
    });
    println!("tokens: {:?}", tokens.collect::<Vec<_>>());

    // repeat_with + take: generate IDs lazily.
    let mut next_id = 1000;
    let ids: Vec<u32> = iter::repeat_with(|| {
        next_id += 1;
        next_id
    })
    .take(3)
    .collect();
    println!("ids: {ids:?}");
}
