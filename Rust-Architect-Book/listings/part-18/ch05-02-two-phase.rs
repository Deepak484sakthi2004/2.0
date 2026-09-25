// verify: debug build
// Source for the MIR artifact in Chapter 18.5: `v.push(v.len())` compiles because the autoref'd
// `&mut *v` is a TWO-PHASE borrow: reserved first, activated only at the call.
#[inline(never)]
pub fn push_len(v: &mut Vec<usize>) {
    v.push(v.len());
}
