// verify: debug crash overflowed its stack
// verify: release crash overflowed its stack
use std::thread;

fn chain(n: usize) -> Vec<Vec<u32>> {
    (0..n).map(|i| if i + 1 < n { vec![i as u32 + 1] } else { vec![] }).collect()
}

/// Same function as intl-01-frame-size.rs.
#[inline(never)]
fn dfs(adj: &[Vec<u32>], node: u32, visited: &mut [bool], depth: usize, probe: &mut [usize; 2]) -> usize {
    let marker = 0u8;
    if depth == 0 {
        probe[0] = &marker as *const u8 as usize;
    } else if depth == 1000 {
        probe[1] = &marker as *const u8 as usize;
    }
    visited[node as usize] = true;
    let mut deepest = depth;
    for &next in &adj[node as usize] {
        if !visited[next as usize] {
            deepest = deepest.max(dfs(adj, next, visited, depth + 1, probe));
        }
    }
    deepest
}

fn run_on_thread(stack_bytes: usize, n: usize) -> (usize, usize) {
    thread::Builder::new()
        .stack_size(stack_bytes)
        .spawn(move || {
            let adj = chain(n);
            let mut visited = vec![false; n];
            let mut probe = [0usize; 2];
            let deepest = dfs(&adj, 0, &mut visited, 0, &mut probe);
            (deepest, (probe[0] - probe[1]) / 1000)
        })
        .unwrap()
        .join()
        .unwrap()
}

fn main() {
    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    let (_, frame) = run_on_thread(2 << 20, 2000);
    let max_2m = (2 << 20) / frame;
    let over = max_2m * 11 / 10;
    println!("[{profile}] frame {frame} bytes, predicted max ~{max_2m}; trying a chain of {over} (110%)");
    let (deepest, _) = run_on_thread(2 << 20, over);
    println!("unreachable: reached depth {deepest}");
}
