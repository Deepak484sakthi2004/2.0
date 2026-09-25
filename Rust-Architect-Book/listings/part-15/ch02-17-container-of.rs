// verify: debug ok
// verify: debug miri Undefined
// verify: debug+tree miri-ok
// `container_of`: from a pointer to a FIELD back to the struct that contains it (intrusive lists do this).
// Here the field pointer comes from a REFERENCE to the field: Stacked Borrows says it may only access
// the field's own bytes; Tree Borrows lets it read the rest of the (still unaliased) struct.
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
    let lp: *const Link = &e.link; // a reference to the FIELD, then a raw pointer
    let ep = entry_of(lp);
    println!("key = {}", unsafe { (*ep).key }); // reads bytes outside `link`
}
