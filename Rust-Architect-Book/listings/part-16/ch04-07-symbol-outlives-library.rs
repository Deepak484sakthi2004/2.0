// verify: debug error:E0505
// "A symbol must not be used after its library is unloaded" as a lifetime: Symbol<'lib, F> borrows
// the Library, so unloading it while a symbol is still in use doesn't compile.
use std::ffi::{CStr, c_char, c_void};
use std::marker::PhantomData;
use std::ptr::NonNull;

pub struct Library {
    handle: NonNull<c_void>,
}

pub struct Symbol<'lib, F> {
    f: F,
    _lib: PhantomData<&'lib Library>,
}

impl Library {
    pub fn open(name: &CStr) -> Option<Library> {
        // SAFETY: NUL-terminated name.
        NonNull::new(unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW) }).map(|handle| Library { handle })
    }

    /// # Safety
    /// `F` must be the symbol's exact function-pointer type.
    pub unsafe fn get<F: Copy>(&self, name: &CStr) -> Option<Symbol<'_, F>> {
        // SAFETY: a live handle and a NUL-terminated name.
        let p = unsafe { libc::dlsym(self.handle.as_ptr(), name.as_ptr()) };
        // SAFETY: the caller's contract on F.
        (!p.is_null()).then(|| Symbol { f: unsafe { std::mem::transmute_copy::<*mut c_void, F>(&p) }, _lib: PhantomData })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: a live handle, closed once.
        unsafe { libc::dlclose(self.handle.as_ptr()) };
    }
}

type StrlenFn = unsafe extern "C" fn(*const c_char) -> usize;

fn main() {
    let lib = Library::open(c"libc.so.6").expect("dlopen");
    // SAFETY: strlen's exact C type.
    let strlen = unsafe { lib.get::<StrlenFn>(c"strlen") }.expect("dlsym");
    drop(lib); // "we're done with the library"...
    // SAFETY (intended): a NUL-terminated literal.
    println!("{}", unsafe { (strlen.f)(c"meridian".as_ptr()) }); // ...but not with its code
}
