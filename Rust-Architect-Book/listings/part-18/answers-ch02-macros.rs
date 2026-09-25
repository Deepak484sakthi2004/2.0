// verify: debug ok
// Answer-key check for Chapter 18.2's debugging exercise and intermediate exercise: macros whose paths
// can't be hijacked by the call site, tested from a module that defines colliding names.
mod metrics {
    pub fn record(name: &str, elapsed: std::time::Duration) {
        println!("metric {name} recorded (under a second: {})", elapsed.as_secs() < 1);
    }

    #[macro_export]
    macro_rules! timed {
        ($name:expr, $body:block) => {{
            let start = ::std::time::Instant::now();
            let out = $body;
            $crate::metrics::record($name, start.elapsed());
            out
        }};
    }

    #[macro_export]
    macro_rules! retry {
        ($n:expr, $op:expr) => {{
            let mut attempt = 0;
            loop {
                attempt += 1;
                let result = $op;
                if result.is_ok() || attempt >= $n {
                    break result;
                }
                ::std::thread::sleep(::std::time::Duration::from_millis(1));
            }
        }};
    }
}

mod caller {
    use crate::{retry, timed};

    // Deliberately colliding names: none of these may be used by the macros.
    #[allow(dead_code)]
    struct Instant;
    #[allow(dead_code)]
    fn record(_: &str, _: std::time::Duration) {
        panic!("hijacked!");
    }
    #[allow(dead_code)]
    fn sleep(_: u64) {
        panic!("hijacked!");
    }

    pub fn run() {
        let attempt = "caller's attempt";
        let result = "caller's result";
        let mut calls = 0;
        let v = timed!("parse", { 40 + 2 });
        let r: Result<u32, &str> = retry!(3, {
            calls += 1;
            if calls < 2 { Err("busy") } else { Ok(v) }
        });
        println!("v={v} r={r:?} calls={calls} ({attempt}, {result})");
    }
}

fn main() {
    caller::run();
}
