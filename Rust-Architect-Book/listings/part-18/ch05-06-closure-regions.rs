// verify: debug+nightly ok
// Nightly-only: #[rustc_regions] prints the region requirements a closure body PROPAGATES to its
// creator. The closure is borrow-checked first; what it needs ('?1: '?3) is proven in `collect`.
#![feature(rustc_attrs)]
#![allow(internal_features)]

#[rustc_regions]
fn collect<'a>(src: &'a [String], out: &mut Vec<&'a str>) {
    src.iter().for_each(|s| out.push(s.as_str()));
}

fn main() {
    let src = vec!["a".to_string()];
    let mut out = Vec::new();
    collect(&src, &mut out);
    println!("{out:?}");
}
