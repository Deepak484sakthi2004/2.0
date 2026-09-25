// verify: release build
//! What does lock-increment-unlock compile to? (release asm via tools/emit.ps1)
use std::sync::Mutex;

#[inline(never)]
pub fn bump(m: &Mutex<u64>) {
    *m.lock().unwrap() += 1;
}
