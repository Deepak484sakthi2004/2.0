// verify: release ok
//! Level-synchronous parallel BFS (promised in the Part IX interlude): expand a whole frontier in parallel,
//! claim each newly reached node with one compare-and-swap, then move to the next level.
use rayon::prelude::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

const UNSEEN: u32 = u32::MAX;

/// Compressed sparse rows: node i's neighbors are edges[offsets[i]..offsets[i + 1]].
struct Graph {
    offsets: Vec<usize>,
    edges: Vec<u32>,
}

impl Graph {
    fn random(n: usize, degree: usize) -> Graph {
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let mut offsets = Vec::with_capacity(n + 1);
        let mut edges = Vec::with_capacity(n * (degree + 1));
        for i in 0..n {
            offsets.push(edges.len());
            edges.push(((i + 1) % n) as u32); // a ring keeps everything reachable
            for _ in 0..degree {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                edges.push((x % n as u64) as u32);
            }
        }
        offsets.push(edges.len());
        Graph { offsets, edges }
    }

    fn neighbors(&self, u: u32) -> &[u32] {
        &self.edges[self.offsets[u as usize]..self.offsets[u as usize + 1]]
    }

    fn len(&self) -> usize {
        self.offsets.len() - 1
    }
}

fn bfs_sequential(g: &Graph, src: u32) -> Vec<u32> {
    let mut dist = vec![UNSEEN; g.len()];
    let mut queue = VecDeque::from([src]);
    dist[src as usize] = 0;
    while let Some(u) = queue.pop_front() {
        for &v in g.neighbors(u) {
            if dist[v as usize] == UNSEEN {
                dist[v as usize] = dist[u as usize] + 1;
                queue.push_back(v);
            }
        }
    }
    dist
}

fn bfs_parallel(g: &Graph, src: u32) -> (Vec<u32>, Vec<usize>) {
    let dist: Vec<AtomicU32> = (0..g.len()).map(|_| AtomicU32::new(UNSEEN)).collect();
    dist[src as usize].store(0, Ordering::Relaxed);
    let mut frontier = vec![src];
    let mut level_sizes = Vec::new();
    let mut level = 0;
    while !frontier.is_empty() {
        level_sizes.push(frontier.len());
        // Many threads may reach the same node; exactly one compare_exchange wins and adds it.
        // Relaxed suffices: the level barrier (collect waits for every task) orders levels (Part XIV).
        frontier = frontier
            .par_iter()
            .flat_map_iter(|&u| {
                g.neighbors(u).iter().copied().filter(|&v| {
                    dist[v as usize].compare_exchange(UNSEEN, level + 1, Ordering::Relaxed, Ordering::Relaxed).is_ok()
                })
            })
            .collect();
        level += 1;
    }
    (dist.into_iter().map(AtomicU32::into_inner).collect(), level_sizes)
}

fn main() {
    let g = Graph::random(2_000_000, 8);
    println!("graph: {} nodes, {} edges; rayon threads: {}", g.len(), g.edges.len(), rayon::current_num_threads());

    let t = Instant::now();
    let seq = bfs_sequential(&g, 0);
    let t_seq = t.elapsed();

    let t = Instant::now();
    let (par, levels) = bfs_parallel(&g, 0);
    let t_par = t.elapsed();

    println!("same distances: {}", seq == par);
    println!("frontier size per level: {levels:?}");
    println!("sequential {t_seq:.1?}, parallel {t_par:.1?} (one run)");
}
