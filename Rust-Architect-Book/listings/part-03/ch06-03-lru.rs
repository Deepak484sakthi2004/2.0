// verify: debug ok
use std::collections::HashMap;

const NIL: usize = usize::MAX;

struct Entry<V> {
    key: u64,
    value: V,
    prev: usize, // links are INDICES into `entries`, not pointers
    next: usize,
}

/// A least-recently-used cache: a doubly linked list threaded through a Vec (the arena),
/// plus a HashMap from key to slot. No Rc, no RefCell, no unsafe.
pub struct Lru<V> {
    map: HashMap<u64, usize>,
    entries: Vec<Entry<V>>,
    head: usize, // most recently used
    tail: usize, // least recently used
    capacity: usize,
}

impl<V> Lru<V> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity > 0, "capacity must be positive");
        Lru { map: HashMap::with_capacity(capacity), entries: Vec::with_capacity(capacity), head: NIL, tail: NIL, capacity }
    }

    fn unlink(&mut self, i: usize) {
        let (prev, next) = (self.entries[i].prev, self.entries[i].next);
        if prev != NIL { self.entries[prev].next = next } else { self.head = next }
        if next != NIL { self.entries[next].prev = prev } else { self.tail = prev }
    }

    fn push_front(&mut self, i: usize) {
        self.entries[i].prev = NIL;
        self.entries[i].next = self.head;
        if self.head != NIL {
            self.entries[self.head].prev = i;
        }
        self.head = i;
        if self.tail == NIL {
            self.tail = i;
        }
    }

    pub fn get(&mut self, key: u64) -> Option<&V> {
        let i = *self.map.get(&key)?;
        self.unlink(i);
        self.push_front(i);
        Some(&self.entries[i].value)
    }

    /// Inserts or updates; returns the evicted entry, if any.
    pub fn put(&mut self, key: u64, value: V) -> Option<(u64, V)> {
        if let Some(&i) = self.map.get(&key) {
            self.entries[i].value = value;
            self.unlink(i);
            self.push_front(i);
            return None;
        }
        if self.entries.len() < self.capacity {
            self.entries.push(Entry { key, value, prev: NIL, next: NIL });
            let i = self.entries.len() - 1;
            self.push_front(i);
            self.map.insert(key, i);
            return None;
        }
        // Full: reuse the least-recently-used slot in place (no allocation per eviction).
        let i = self.tail;
        self.unlink(i);
        let old = std::mem::replace(&mut self.entries[i], Entry { key, value, prev: NIL, next: NIL });
        self.map.remove(&old.key);
        self.map.insert(key, i);
        self.push_front(i);
        Some((old.key, old.value))
    }

    pub fn keys_mru_to_lru(&self) -> Vec<u64> {
        let mut keys = Vec::with_capacity(self.entries.len());
        let mut i = self.head;
        while i != NIL {
            keys.push(self.entries[i].key);
            i = self.entries[i].next;
        }
        keys
    }
}

fn main() {
    let mut sessions: Lru<&str> = Lru::new(3);
    sessions.put(1, "ada");
    sessions.put(2, "grace");
    sessions.put(3, "linus");
    println!("order (MRU..LRU): {:?}", sessions.keys_mru_to_lru());
    println!("get(1) = {:?}", sessions.get(1));
    println!("order (MRU..LRU): {:?}", sessions.keys_mru_to_lru());
    println!("put(4) evicted {:?}", sessions.put(4, "ken"));
    println!("order (MRU..LRU): {:?}", sessions.keys_mru_to_lru());
    println!("get(2) = {:?}", sessions.get(2));
}
