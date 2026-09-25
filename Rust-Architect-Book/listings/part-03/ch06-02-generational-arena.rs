// verify: debug ok
/// A handle into an Arena: an index plus the generation it was issued for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    index: u32,
    generation: u32,
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

/// Owns every value; hands out Copy keys instead of references.
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    len: usize,
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Arena { slots: Vec::new(), free: Vec::new(), len: 0 }
    }

    pub fn insert(&mut self, value: T) -> Key {
        self.len += 1;
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.value = Some(value);
            Key { index, generation: slot.generation }
        } else {
            self.slots.push(Slot { generation: 0, value: Some(value) });
            Key { index: (self.slots.len() - 1) as u32, generation: 0 }
        }
    }

    pub fn get(&self, key: Key) -> Option<&T> {
        let slot = self.slots.get(key.index as usize)?;
        if slot.generation == key.generation { slot.value.as_ref() } else { None }
    }

    pub fn get_mut(&mut self, key: Key) -> Option<&mut T> {
        let slot = self.slots.get_mut(key.index as usize)?;
        if slot.generation == key.generation { slot.value.as_mut() } else { None }
    }

    pub fn remove(&mut self, key: Key) -> Option<T> {
        let slot = self.slots.get_mut(key.index as usize)?;
        if slot.generation != key.generation {
            return None;
        }
        let value = slot.value.take()?;
        slot.generation = slot.generation.wrapping_add(1); // every outstanding key to this slot goes stale
        self.free.push(key.index);
        self.len -= 1;
        Some(value)
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

fn main() {
    let mut sessions: Arena<String> = Arena::new();
    let alice = sessions.insert("alice".to_string());
    let bob = sessions.insert("bob".to_string());
    println!("alice = {alice:?}");
    println!("bob   = {bob:?}");

    println!("remove(alice) = {:?}", sessions.remove(alice));
    let carol = sessions.insert("carol".to_string()); // reuses alice's slot, with a new generation
    println!("carol = {carol:?}");

    println!("get(alice) = {:?}   <- stale key detected, not carol's data", sessions.get(alice));
    println!("get(carol) = {:?}", sessions.get(carol));
    if let Some(name) = sessions.get_mut(bob) {
        name.push_str(" (vip)");
    }
    println!("get(bob)   = {:?}", sessions.get(bob));
    println!("len = {}", sessions.len());
}
