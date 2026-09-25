// verify: debug ok
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Top-k largest with a bounded MIN-heap of size k: O(n log k) time, O(k) memory.
fn top_k(values: impl IntoIterator<Item = u64>, k: usize) -> Vec<u64> {
    let mut heap: BinaryHeap<Reverse<u64>> = BinaryHeap::with_capacity(k + 1);
    for v in values {
        if heap.len() < k {
            heap.push(Reverse(v));
        } else if let Some(&Reverse(smallest)) = heap.peek() {
            if v > smallest {
                heap.pop();
                heap.push(Reverse(v));
            }
        }
    }
    let mut out: Vec<u64> = heap.into_iter().map(|Reverse(v)| v).collect();
    out.sort_unstable_by(|a, b| b.cmp(a));
    out
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Job {
    deadline_ms: u64, // compared first: field order is the ordering
    id: u32,
}

fn main() {
    let latencies = [120, 45, 3000, 88, 910, 15, 4100, 230, 77, 2600];
    println!("top 3 latencies: {:?}", top_k(latencies, 3));

    // Earliest-deadline-first: BinaryHeap is a max-heap, so wrap in Reverse.
    let mut edf = BinaryHeap::new();
    edf.push(Reverse(Job { deadline_ms: 250, id: 1 }));
    edf.push(Reverse(Job { deadline_ms: 100, id: 2 }));
    edf.push(Reverse(Job { deadline_ms: 175, id: 3 }));
    edf.push(Reverse(Job { deadline_ms: 100, id: 0 }));
    let order: Vec<(u64, u32)> = std::iter::from_fn(|| edf.pop()).map(|Reverse(j)| (j.deadline_ms, j.id)).collect();
    println!("EDF order (deadline, id): {order:?}");

    // The heap's backing store is a Vec in level order; peek is O(1), push/pop O(log n).
    let h: BinaryHeap<u32> = [5, 1, 8, 3, 9, 2].into_iter().collect();
    println!("heap as stored (level order): {:?}", h.as_slice());
    println!("into_sorted_vec: {:?}", h.into_sorted_vec());
}
