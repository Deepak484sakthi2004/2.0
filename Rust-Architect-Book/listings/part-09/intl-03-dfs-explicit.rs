// verify: debug ok
// verify: release ok
use std::thread;

fn chain(n: usize) -> Vec<Vec<u32>> {
    (0..n).map(|i| if i + 1 < n { vec![i as u32 + 1] } else { vec![] }).collect()
}

const NONE: u32 = u32::MAX;

/// Explicit-stack DFS that mirrors recursion exactly: one heap "frame" = (node, next edge index).
/// Returns (parent of each node in the DFS tree, post-order, peak stack length).
fn dfs_frames(adj: &[Vec<u32>], start: u32) -> (Vec<u32>, Vec<u32>, usize) {
    let mut parent = vec![NONE; adj.len()];
    let mut visited = vec![false; adj.len()];
    let mut post = Vec::with_capacity(adj.len());
    let mut stack: Vec<(u32, u32)> = vec![(start, 0)];
    visited[start as usize] = true;
    let mut peak = 1;
    while let Some(top) = stack.last_mut() {
        let (node, edge) = *top;
        match adj[node as usize].get(edge as usize) {
            Some(&next) => {
                top.1 += 1; // resume point: like the saved instruction pointer of a real frame
                if !visited[next as usize] {
                    visited[next as usize] = true;
                    parent[next as usize] = node;
                    stack.push((next, 0));
                    peak = peak.max(stack.len());
                }
            }
            None => {
                post.push(node); // all children done: post-order position (topological sort needs this)
                stack.pop();
            }
        }
    }
    (parent, post, peak)
}

/// The common shortcut: a stack of nodes, marking on push. Visits every reachable node once,
/// but it is NOT the recursive DFS: the tree (parents) and the post-order differ.
fn dfs_mark_on_push(adj: &[Vec<u32>], start: u32) -> Vec<u32> {
    let mut parent = vec![NONE; adj.len()];
    let mut visited = vec![false; adj.len()];
    let mut stack = vec![start];
    visited[start as usize] = true;
    while let Some(node) = stack.pop() {
        for &next in adj[node as usize].iter().rev() {
            if !visited[next as usize] {
                visited[next as usize] = true;
                parent[next as usize] = node;
                stack.push(next);
            }
        }
    }
    parent
}

#[inline(never)]
fn dfs_recursive(adj: &[Vec<u32>], node: u32, visited: &mut [bool]) -> usize {
    visited[node as usize] = true;
    let mut count = 1;
    for &next in &adj[node as usize] {
        if !visited[next as usize] {
            count += dfs_recursive(adj, next, visited);
        }
    }
    count
}

fn main() {
    // 1. A million-deep chain on a 2 MiB thread: fine, because the "stack" is a heap Vec.
    let (reached, peak) = thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            let adj = chain(1_000_000);
            let (_, post, peak) = dfs_frames(&adj, 0);
            (post.len(), peak)
        })
        .unwrap()
        .join()
        .unwrap();
    println!("explicit stack, 1,000,000-node chain, 2 MiB thread: visited {reached}, peak stack {peak} frames = {} KiB of heap", peak * 8 / 1024);

    // 2. Or keep the recursion and give the thread a bigger stack (the stack size is a deployment parameter).
    let visited = thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            let adj = chain(200_000);
            let mut visited = vec![false; adj.len()];
            dfs_recursive(&adj, 0, &mut visited)
        })
        .unwrap()
        .join()
        .unwrap();
    println!("recursive, 200,000-node chain, 64 MiB thread: visited {visited}");

    // 3. Why the (node, edge) frame matters: 0 -> {1, 3}, 1 -> {2}, 2 -> {3}.
    let g = vec![vec![1, 3], vec![2], vec![3], vec![]];
    let (parent, post, _) = dfs_frames(&g, 0);
    println!("frame-stack DFS: parents {:?}, post-order {post:?}", show(&parent));
    println!("mark-on-push:    parents {:?}", show(&dfs_mark_on_push(&g, 0)));
}

fn show(parent: &[u32]) -> Vec<Option<u32>> {
    parent.iter().map(|&p| (p != NONE).then_some(p)).collect()
}
