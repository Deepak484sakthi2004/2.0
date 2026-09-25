// verify: debug ok
// Which of Fn / FnMut / FnOnce does each closure implement? The compiler decides from the BODY:
// what the body does with each capture, not how the capture was taken (`move` or not).

fn call_fn(f: impl Fn() -> usize) -> usize {
    f() + f() // may call any number of times, through &self
}
fn call_fn_mut(mut f: impl FnMut() -> usize) -> usize {
    f() + f() // may call any number of times, through &mut self
}
fn call_fn_once(f: impl FnOnce() -> usize) -> usize {
    f() // may call at most once: calling consumes self
}

fn main() {
    let routes = vec!["/pay".to_string(), "/refund".to_string()];

    // Only READS its captures -> Fn (and therefore also FnMut and FnOnce).
    let count = || routes.len();
    println!("Fn     via call_fn:      {}", call_fn(count));
    println!("Fn     via call_fn_mut:  {}", call_fn_mut(count));
    println!("Fn     via call_fn_once: {}", call_fn_once(count));

    // MUTATES a capture -> FnMut (and FnOnce), but not Fn.
    let mut calls = 0;
    let bump = || {
        calls += 1;
        calls
    };
    println!("FnMut  via call_fn_mut:  {}", call_fn_mut(bump));

    // MOVES a capture out of itself -> FnOnce only.
    let hand_off = move || {
        let owned: Vec<String> = routes; // moves the captured Vec out of the closure
        owned.len()
    };
    println!("FnOnce via call_fn_once: {}", call_fn_once(hand_off));

    // `move` changes HOW captures are taken, not which trait is implemented:
    let limit = 3usize;
    let reads_moved = move || limit * 2; // owns its copy of `limit`, only reads it -> still Fn
    println!("move + read-only is Fn:  {}", call_fn(reads_moved));
    println!("calls after bump: {calls}");
}
