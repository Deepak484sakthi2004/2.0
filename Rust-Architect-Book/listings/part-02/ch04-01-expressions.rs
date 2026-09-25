// verify: debug ok
fn classify(latency_ms: u32) -> &'static str {
    // `if` is an expression; every branch must produce the same type.
    if latency_ms < 100 {
        "fast"
    } else if latency_ms < 1_000 {
        "slow"
    } else {
        "timeout-risk"
    }
}

fn first_over(limit: u32, samples: &[u32]) -> Option<usize> {
    let mut i = 0;
    // `loop` is an expression too: `break value` produces its result.
    loop {
        if i == samples.len() {
            break None;
        }
        if samples[i] > limit {
            break Some(i);
        }
        i += 1;
    }
}

fn min_max(samples: &[u32]) -> (u32, u32) {
    // A block evaluates to its tail expression (the last line, without a semicolon).
    let min = {
        let mut m = u32::MAX;
        for &s in samples {
            m = m.min(s);
        }
        m
    };
    let max = samples.iter().copied().max().unwrap_or(0);
    (min, max) // a tuple: an anonymous product type
}

fn main() {
    let samples = [120, 45, 3_000, 80]; // [u32; 4]: the length is part of the type
    for s in samples {
        println!("{s:>5} ms -> {}", classify(s));
    }
    let (lo, hi) = min_max(&samples); // destructuring the tuple
    println!("min={lo} max={hi}");
    println!("first over 1000 at index {:?}", first_over(1_000, &samples));

    // A labeled block: `break 'label value` exits it with a value.
    let grid = [[1, 2, 3], [4, 99, 6]];
    let pos = 'search: {
        for (r, row) in grid.iter().enumerate() {
            for (c, &v) in row.iter().enumerate() {
                if v == 99 {
                    break 'search Some((r, c));
                }
            }
        }
        None
    };
    println!("99 found at {pos:?}");

    // `!` (never): panic!, return, continue, and `loop {}` produce no value, so they fit any type.
    let parsed: u32 = match "17".parse() {
        Ok(n) => n,
        Err(_) => panic!("not a number"),
    };
    println!("parsed={parsed}");
}
