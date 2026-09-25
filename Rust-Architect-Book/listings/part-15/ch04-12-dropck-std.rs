// verify: debug ok
// The same shape as ch04-11-dropck.rs with std's Vec: it compiles, because Vec's Drop impl is
// declared `unsafe impl<#[may_dangle] T, A: Allocator> Drop for Vec<T, A>`: it promises not to
// touch its T's in drop except to drop them, and dropping a `&String` does nothing.
fn main() {
    let mut names: Vec<&String> = Vec::new();
    let owner = String::from("alice"); // dropped before `names`
    names.push(&owner);
    println!("{}", names.len());
}
