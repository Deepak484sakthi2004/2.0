// verify: debug ok
// verify: debug miri-ok
// One Java String, three byte encodings at the boundary (Chapter 9.2's incident, from the C side).
// The entry points validate and return MERIDIAN_ERR_INVALID; they never "repair" the text.
use std::ffi::{CStr, c_char};

pub const MERIDIAN_OK: i32 = 0;
pub const MERIDIAN_ERR_INVALID: i32 = -1;

/// The name feature: FNV-1a over the UTF-8 bytes. Any change to the bytes changes the feature.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

fn finish(text: Option<String>, out: *mut u64) -> i32 {
    match (text, out.is_null()) {
        (Some(s), false) => {
            // SAFETY: the contract: `out` is writable (NULL was checked just above).
            unsafe { out.write(fnv1a(s.as_bytes())) };
            MERIDIAN_OK
        }
        _ => MERIDIAN_ERR_INVALID,
    }
}

/// FFM: `arena.allocateFrom(name)` passes NUL-terminated standard UTF-8.
/// # Safety
/// `name` is NULL or a NUL-terminated string valid for the call; `out` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_name_hash(name: *const c_char, out: *mut u64) -> i32 {
    if name.is_null() {
        return MERIDIAN_ERR_INVALID;
    }
    // SAFETY: the contract: non-null and NUL-terminated.
    let bytes = unsafe { CStr::from_ptr(name) }.to_bytes();
    finish(std::str::from_utf8(bytes).ok().map(str::to_owned), out)
}

/// JNI `GetStringChars`: UTF-16 code units, (pointer, length), no terminator.
/// # Safety
/// `units` points to `len` readable u16s (or len == 0); `out` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn meridian_name_hash_utf16(units: *const u16, len: usize, out: *mut u64) -> i32 {
    let units: &[u16] = if len == 0 {
        &[]
    } else if units.is_null() || !units.is_aligned() {
        return MERIDIAN_ERR_INVALID;
    } else {
        // SAFETY: the contract: `len` readable u16s at a non-null, aligned `units`.
        unsafe { std::slice::from_raw_parts(units, len) }
    };
    finish(String::from_utf16(units).ok(), out)
}

/// What JNI's GetStringUTFChars produces: "Modified UTF-8" (U+0000 as C0 80, and every
/// supplementary character as two 3-byte surrogate encodings instead of one 4-byte sequence).
fn to_modified_utf8(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for unit in s.encode_utf16() {
        match unit {
            0 => out.extend([0xC0, 0x80]),
            1..=0x7F => out.push(unit as u8),
            0x80..=0x7FF => out.extend([0xC0 | (unit >> 6) as u8, 0x80 | (unit & 0x3F) as u8]),
            _ => out.extend([0xE0 | (unit >> 12) as u8, 0x80 | ((unit >> 6) & 0x3F) as u8, 0x80 | (unit & 0x3F) as u8]),
        }
    }
    out
}

fn main() {
    let name = "Zoë 😀";
    let mut h = 0u64;

    let mut ffm = name.as_bytes().to_vec();
    ffm.push(0);
    // SAFETY (all calls below): valid, NUL-terminated or (ptr, len) buffers, and a writable `out`.
    let rc = unsafe { meridian_name_hash(ffm.as_ptr().cast(), &mut h) };
    println!("FFM, standard UTF-8   {:02X?}\n    -> rc={rc} hash={h:016x}", name.as_bytes());

    let utf16: Vec<u16> = name.encode_utf16().collect();
    let rc = unsafe { meridian_name_hash_utf16(utf16.as_ptr(), utf16.len(), &mut h) };
    println!("JNI GetStringChars    {:04X?}\n    -> rc={rc} hash={h:016x}", utf16);

    let mut jni = to_modified_utf8(name);
    println!("JNI GetStringUTFChars {:02X?}", jni);
    jni.push(0);
    let rc = unsafe { meridian_name_hash(jni.as_ptr().cast(), &mut h) };
    println!("    -> rc={rc} (rejected: not UTF-8)");

    let lossy = String::from_utf8_lossy(&jni[..jni.len() - 1]);
    let replaced = lossy.chars().filter(|&c| c == char::REPLACEMENT_CHARACTER).count();
    println!("the lossy \"fix\": {replaced} x U+FFFD instead of the emoji -> hash={:016x}", fnv1a(lossy.as_bytes()));
}
