// verify: debug ok
// verify: debug miri-ok
// verify: debug+tree miri-ok
// The shape of std's get_disjoint_mut: check bounds AND pairwise distinctness, then derive every
// reference from one raw pointer.
#[derive(Debug)]
enum DisjointError {
    IndexOutOfBounds,
    OverlappingIndices,
}

fn get_disjoint<T, const N: usize>(s: &mut [T], idx: [usize; N]) -> Result<[&mut T; N], DisjointError> {
    for (k, &i) in idx.iter().enumerate() {
        if i >= s.len() {
            return Err(DisjointError::IndexOutOfBounds);
        }
        if idx[..k].contains(&i) {
            return Err(DisjointError::OverlappingIndices); // O(N^2): fine for small N
        }
    }
    let p = s.as_mut_ptr();
    // SAFETY: every index is in bounds and all are pairwise distinct (checked above), so the N
    // references point to N different elements. All derive from the one pointer `p`, and their
    // lifetime is tied to the exclusive borrow of `s`.
    Ok(idx.map(|i| unsafe { &mut *p.add(i) }))
}

fn main() {
    let mut balances = [100i64, 50, 70];
    match get_disjoint(&mut balances, [0, 1]) {
        Ok([a, b]) => {
            *a -= 30;
            *b += 30;
        }
        Err(e) => println!("rejected: {e:?}"),
    }
    println!("{balances:?}");
    println!("{:?}", get_disjoint(&mut balances, [2, 2]).map(|_| ()));
    println!("{:?}", get_disjoint(&mut balances, [0, 9]).map(|_| ()));
    if let Ok([x, y, z]) = get_disjoint(&mut balances, [2, 0, 1]) {
        std::mem::swap(x, y);
        *z += 1;
    }
    println!("{balances:?}");
}
