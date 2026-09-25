// verify: debug ok
//! A panic kills only its own thread. join() hands the payload to whoever joins; the process keeps running.
use std::any::Any;
use std::thread;

fn describe(payload: &(dyn Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        format!("&str payload: {s:?}")
    } else if let Some(s) = payload.downcast_ref::<String>() {
        format!("String payload: {s:?}")
    } else {
        "non-string payload".to_string()
    }
}

fn main() {
    // Quiet the default hook so stdout shows just the program's view (the hook would print to stderr).
    std::panic::set_hook(Box::new(|_| {}));

    let ok = thread::spawn(|| 6 * 7);
    let literal = thread::spawn(|| -> u32 { panic!("bad input") });
    let formatted = thread::spawn(|| -> u32 {
        let shard = 3;
        panic!("shard {shard} corrupted")
    });

    println!("ok:        {:?}", ok.join());
    match literal.join() {
        Ok(v) => println!("literal:   {v}"),
        Err(payload) => println!("literal:   Err({})", describe(&*payload)),
    }
    match formatted.join() {
        Ok(v) => println!("formatted: {v}"),
        Err(payload) => println!("formatted: Err({})", describe(&*payload)),
    }

    // A scope joins every thread it spawned; if one panicked and nobody joined it explicitly,
    // the scope itself panics when it ends.
    let r = std::panic::catch_unwind(|| {
        thread::scope(|s| {
            s.spawn(|| panic!("inside scope"));
        });
    });
    println!("scope with an unjoined panicking thread: {}", if r.is_err() { "scope panicked" } else { "ok" });
    println!("main is still running");
}
