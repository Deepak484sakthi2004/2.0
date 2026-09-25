// verify: debug ok
//! The interior-mutability family, measured: what each one adds to the data it wraps (rustc 1.98.1, x86-64).
//! Sizes are [RUSTC]/[LIB] facts, not guarantees (except UnsafeCell<T>, which has T's layout: repr(transparent)).
use std::cell::{Cell, OnceCell, RefCell, UnsafeCell};
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Mutex, OnceLock, RwLock};

macro_rules! sizes {
    ($($t:ty),* $(,)?) => { $( println!("{:<34} {:>3} bytes", stringify!($t), size_of::<$t>()); )* };
}

fn main() {
    sizes!(
        u64,
        UnsafeCell<u64>,
        Cell<u64>,
        RefCell<u64>,
        OnceCell<u64>,
        OnceCell<String>,
        AtomicBool,
        AtomicU64,
        OnceLock<u64>,
        Mutex<()>,
        Mutex<u64>,
        RwLock<()>,
        RwLock<u64>,
        parking_lot::Mutex<()>,
        parking_lot::Mutex<u64>,
        parking_lot::RwLock<u64>,
        Mutex<[u8; 64]>,
    );
}
