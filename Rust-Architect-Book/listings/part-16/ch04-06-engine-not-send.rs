// verify: debug error:E0277
// The thread-affinity rule, enforced by the compiler: a scoring engine can't be moved to another
// thread, because `Engine` contains `PhantomData<*const ()>` and raw pointers are neither Send nor Sync.
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::thread;

#[repr(C)]
pub struct VseEngine {
    _private: [u8; 0],
}

pub struct Engine {
    raw: NonNull<VseEngine>,
    _thread_affine: PhantomData<*const ()>,
}

impl Engine {
    pub fn score(&mut self, features: &[f64]) -> f64 {
        let _ = self.raw;
        features.iter().sum() // stand-in: the real call is vse_score (Chapter 16.2)
    }
}

pub fn score_in_background(mut engine: Engine) -> thread::JoinHandle<f64> {
    thread::spawn(move || engine.score(&[0.9, 0.5, 0.1]))
}

fn main() {}
