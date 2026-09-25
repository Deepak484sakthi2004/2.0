// verify: debug ok
// verify: debug miri-ok
// verify: debug+tree miri-ok
// The portable fix: derive the field pointer from a raw pointer to the WHOLE struct (no reference to the
// field in between), so it keeps the whole struct's permission and `container_of` is fine under both models.
use std::mem::offset_of;

#[repr(C)]
struct Entry {
    key: u64,
    link: Link,
}

#[repr(C)]
struct Link {
    next: *const Link,
}

fn entry_of(link: *const Link) -> *const Entry {
    link.wrapping_byte_sub(offset_of!(Entry, link)).cast::<Entry>()
}

fn main() {
    let e = Entry { key: 7, link: Link { next: std::ptr::null() } };
    let whole: *const Entry = &raw const e;
    // SAFETY: `whole` points to a live Entry; `&raw const` projects to the field without creating a reference.
    let lp: *const Link = unsafe { &raw const (*whole).link };
    let ep = entry_of(lp);
    // SAFETY: `lp` was derived from `whole` (permission for all of `e`), and `ep` points back to the start of
    // that same live, initialized Entry, which nothing is writing.
    println!("key = {}", unsafe { (*ep).key });
}
