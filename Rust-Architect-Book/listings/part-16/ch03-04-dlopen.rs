// verify: debug ok
// How a host finds C-ABI symbols at run time: what the JVM's SymbolLookup (FFM) and System.loadLibrary
// (JNI) do, and what a Rust plugin host does with a cdylib. A symbol borrows the library it came from.
use std::ffi::{CStr, c_char, c_void};
use std::marker::PhantomData;
use std::ops::Deref;
use std::ptr::NonNull;

/// INVARIANT: `handle` came from a successful dlopen and is closed exactly once, in Drop.
pub struct Library {
    handle: NonNull<c_void>,
}

/// A function pointer that can't outlive the library it points into.
pub struct Symbol<'lib, F> {
    f: F,
    _lib: PhantomData<&'lib Library>,
}

impl<F> Deref for Symbol<'_, F> {
    type Target = F;
    fn deref(&self) -> &F {
        &self.f
    }
}

fn dl_error() -> String {
    // SAFETY: dlerror has no preconditions; a non-null result is a NUL-terminated message that stays
    // valid until the next dl* call on this thread, and it's copied immediately.
    let p = unsafe { libc::dlerror() };
    if p.is_null() {
        return "no error recorded".to_owned();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}

impl Library {
    pub fn open(name: &CStr) -> Result<Library, String> {
        // SAFETY: `name` is NUL-terminated. (Loading runs the library's initializers: only load
        // libraries you trust, exactly as with System.loadLibrary.)
        let h = unsafe { libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        NonNull::new(h).map(|handle| Library { handle }).ok_or_else(dl_error)
    }

    /// The running executable itself (dlopen(NULL)).
    pub fn this_program() -> Result<Library, String> {
        // SAFETY: a NULL name is documented to mean "the main program".
        let h = unsafe { libc::dlopen(std::ptr::null(), libc::RTLD_NOW) };
        NonNull::new(h).map(|handle| Library { handle }).ok_or_else(dl_error)
    }

    /// # Safety
    /// `F` must be a function-pointer type exactly matching the symbol's C signature and ABI.
    pub unsafe fn get<F: Copy>(&self, name: &CStr) -> Result<Symbol<'_, F>, String> {
        assert_eq!(size_of::<F>(), size_of::<*mut c_void>(), "F must be a function pointer");
        // SAFETY: a live handle (the invariant) and a NUL-terminated name.
        let p = unsafe { libc::dlsym(self.handle.as_ptr(), name.as_ptr()) };
        if p.is_null() {
            return Err(dl_error());
        }
        // SAFETY: the caller guarantees that F is the symbol's exact function-pointer type.
        let f = unsafe { std::mem::transmute_copy::<*mut c_void, F>(&p) };
        Ok(Symbol { f, _lib: PhantomData })
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        // SAFETY: the invariant; no Symbol can outlive `self` (they borrow it).
        unsafe { libc::dlclose(self.handle.as_ptr()) };
    }
}

/// Exported with an unmangled name... but only a cdylib puts it in the dynamic symbol table.
#[unsafe(no_mangle)]
pub extern "C" fn meridian_abi_version() -> u32 {
    3
}

type StrlenFn = unsafe extern "C" fn(*const c_char) -> usize;
type VersionFn = extern "C" fn() -> u32;

fn main() -> Result<(), String> {
    let libc_so = Library::open(c"libc.so.6")?;
    // SAFETY: strlen's C signature is `size_t strlen(const char *)`.
    let strlen = unsafe { libc_so.get::<StrlenFn>(c"strlen")? };
    // SAFETY: a NUL-terminated literal.
    println!("strlen via dlsym(libc.so.6) = {}", unsafe { (*strlen)(c"meridian".as_ptr()) });

    let me = Library::this_program()?;
    println!("direct call: meridian_abi_version() = {}", meridian_abi_version());
    // SAFETY: the declared type matches the definition above.
    match unsafe { me.get::<VersionFn>(c"meridian_abi_version") } {
        Ok(f) => println!("dlsym(this program) found it: {}", (*f)()),
        Err(e) => println!("dlsym(this program) failed: {e}"),
    }
    Ok(())
}
