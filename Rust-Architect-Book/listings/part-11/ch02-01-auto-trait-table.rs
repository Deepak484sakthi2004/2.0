// verify: debug ok
//! Which types are Send, which are Sync? Asked of the compiler itself, for concrete types.
//!
//! The trick: an inherent associated const wins over a trait's const of the same name, but only exists when
//! the inherent impl's bound holds. So `Probe<T>::SEND` is `true` exactly when `T: Send` (for a concrete T).
use std::cell::{Cell, OnceCell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, RwLock};

struct Probe<T: ?Sized>(PhantomData<T>);

trait NotSend {
    const SEND: bool = false;
}
impl<T: ?Sized> NotSend for Probe<T> {}
impl<T: ?Sized + Send> Probe<T> {
    const SEND: bool = true;
}

struct ProbeSync<T: ?Sized>(PhantomData<T>);
trait NotSync {
    const SYNC: bool = false;
}
impl<T: ?Sized> NotSync for ProbeSync<T> {}
impl<T: ?Sized + Sync> ProbeSync<T> {
    const SYNC: bool = true;
}

macro_rules! row {
    ($($t:ty),* $(,)?) => {
        $(
            println!(
                "{:<30} {:<6} {:<6}",
                stringify!($t),
                if Probe::<$t>::SEND { "Send" } else { "-" },
                if ProbeSync::<$t>::SYNC { "Sync" } else { "-" },
            );
        )*
    };
}

fn main() {
    println!("{:<30} {:<6} {:<6}", "type", "Send?", "Sync?");
    row!(
        u64,
        String,
        Vec<u8>,
        &'static str,
        Box<dyn Fn()>,
        Box<dyn Fn() + Send>,
        Rc<u64>,
        Arc<u64>,
        Cell<u64>,
        RefCell<u64>,
        OnceCell<u64>,
        &'static Cell<u64>,
        &'static mut Cell<u64>,
        Arc<Cell<u64>>,
        Mutex<Cell<u64>>,
        RwLock<Cell<u64>>,
        Mutex<Rc<u64>>,
        MutexGuard<'static, u64>,
        OnceLock<u64>,
        AtomicU64,
        Sender<u64>,
        Receiver<u64>,
        *const u8,
        PhantomData<*const ()>,
    );
}
