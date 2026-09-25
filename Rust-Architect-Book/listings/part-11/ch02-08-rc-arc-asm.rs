// verify: release build
//! Rc::clone vs Arc::clone in release assembly: the only difference Send/Sync buys at run time.
use std::rc::Rc;
use std::sync::Arc;

#[inline(never)]
pub fn clone_rc(r: &Rc<u64>) -> Rc<u64> {
    Rc::clone(r)
}

#[inline(never)]
pub fn clone_arc(a: &Arc<u64>) -> Arc<u64> {
    Arc::clone(a)
}
