// verify: debug ok
// verify: release ok
use std::thread;

/// A chain graph 0 -> 1 -> 2 -> ... -> n-1: the worst case for recursion depth.
fn chain(n: usize) -> Vec<Vec<u32>> {
    (0..n).map(|i| if i + 1 < n { vec![i as u32 + 1] } else { vec![] }).collect()
}

/// Recursive DFS. Returns the deepest depth reached. `probe` records the address of a
/// local variable at depth 0 and depth 1000, which gives the size of one stack frame.
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
    let max_8m = (8 << 20) / frame;
    println!("[{profile}] one dfs() frame = {frame} bytes");
    println!("[{profile}] predicted max depth: ~{max_2m} on a 2 MiB thread, ~{max_8m} on an 8 MiB stack");

    let safe = max_2m * 9 / 10;
    let (deepest, _) = run_on_thread(2 << 20, safe);
    println!("[{profile}] chain of {safe} nodes (90% of prediction) on a 2 MiB thread: ok, reached depth {deepest}");
}
