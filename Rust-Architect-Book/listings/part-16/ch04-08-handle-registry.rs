// verify: debug ok
// verify: debug miri-ok
// Handles that travel as integers (a JNI `long nativeHandle`, a Java `long` in an FFM struct).
// (A) the address itself: expose the provenance going out, re-acquire it coming back (Chapter 15.2);
// (B) an index + generation into a registry: a stale or repeated handle becomes an error code.
use std::sync::Mutex;

pub const MERIDIAN_OK: i32 = 0;
pub const MERIDIAN_ERR_INVALID: i32 = -1;

pub struct Session {
    txns: u64,
}

// ---------------- (A) pointer-as-integer ----------------

#[unsafe(no_mangle)]
pub extern "C" fn meridian_session_open_addr() -> u64 {
    let p = Box::into_raw(Box::new(Session { txns: 0 }));
    p.expose_provenance() as u64 // the integer may later be turned back into this pointer
}

/// # Safety
/// `h` came from `meridian_session_open_addr`, has not been closed, and isn't used concurrently.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_session_record_addr(h: u64) -> u64 {
    let p = std::ptr::with_exposed_provenance_mut::<Session>(h as usize);
    // SAFETY: the contract: a live, exclusively used Session whose provenance was exposed above.
    let s = unsafe { &mut *p };
    s.txns += 1;
    s.txns
}

/// # Safety
/// As above; `h` is not used again afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_session_close_addr(h: u64) {
    // SAFETY: the contract: from Box::into_raw, closed exactly once.
    drop(unsafe { Box::from_raw(std::ptr::with_exposed_provenance_mut::<Session>(h as usize)) });
}

// ---------------- (B) generational registry ----------------

struct Slot {
    generation: u32,
    value: Option<Session>,
}

static SESSIONS: Mutex<Vec<Slot>> = Mutex::new(Vec::new());

fn handle(index: usize, generation: u32) -> u64 {
    ((generation as u64) << 32) | (index as u64 + 1) // index + 1: the handle 0 is never valid
}

fn split(h: u64) -> Option<(usize, u32)> {
    let index = (h & 0xFFFF_FFFF).checked_sub(1)? as usize;
    Some((index, (h >> 32) as u32))
}

#[unsafe(no_mangle)]
pub extern "C" fn meridian_session_open() -> u64 {
    let mut slots = SESSIONS.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(i) = slots.iter().position(|s| s.value.is_none()) {
        slots[i].generation += 1; // reusing the slot invalidates every older handle to it
        slots[i].value = Some(Session { txns: 0 });
        return handle(i, slots[i].generation);
    }
    slots.push(Slot { generation: 0, value: Some(Session { txns: 0 }) });
    handle(slots.len() - 1, 0)
}

/// Safe to call with ANY integer: unknown, closed, or reused handles are MERIDIAN_ERR_INVALID.
#[unsafe(no_mangle)]
pub extern "C" fn meridian_session_record(h: u64, out_txns: Option<&mut u64>) -> i32 {
    let mut slots = SESSIONS.lock().unwrap_or_else(|p| p.into_inner());
    let Some((i, generation)) = split(h) else { return MERIDIAN_ERR_INVALID };
    match (slots.get_mut(i), out_txns) {
        (Some(Slot { generation: g, value: Some(s) }), Some(out)) if *g == generation => {
            s.txns += 1;
            *out = s.txns;
            MERIDIAN_OK
        }
        _ => MERIDIAN_ERR_INVALID,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn meridian_session_close(h: u64) -> i32 {
    let mut slots = SESSIONS.lock().unwrap_or_else(|p| p.into_inner());
    let Some((i, generation)) = split(h) else { return MERIDIAN_ERR_INVALID };
    match slots.get_mut(i) {
        Some(slot) if slot.generation == generation && slot.value.is_some() => {
            slot.value = None;
            MERIDIAN_OK
        }
        _ => MERIDIAN_ERR_INVALID,
    }
}

fn main() {
    // (A): correct use only: the contract can't be checked, so it isn't exercised any other way.
    let a = meridian_session_open_addr();
    // SAFETY: `a` is live and used by this thread only; closed once at the end.
    unsafe {
        meridian_session_record_addr(a);
        println!("(A) pointer handle: txns = {}", meridian_session_record_addr(a));
        meridian_session_close_addr(a);
    }

    // (B): misuse is part of the contract, and gets an answer.
    let mut n = 0;
    let h = meridian_session_open();
    println!("(B) handle {h:#x}: record -> {} (txns {n})", meridian_session_record(h, Some(&mut n)));
    println!("    close -> {}, close again -> {}", meridian_session_close(h), meridian_session_close(h));
    println!("    record after close -> {}", meridian_session_record(h, Some(&mut n)));
    let h2 = meridian_session_open();
    println!("    reopened slot: new handle {h2:#x}; old handle -> {}, new -> {}",
        meridian_session_record(h, Some(&mut n)), meridian_session_record(h2, Some(&mut n)));
    println!("    handle 0 -> {}, handle 12345 -> {}", meridian_session_record(0, Some(&mut n)), meridian_session_close(12345));
}
