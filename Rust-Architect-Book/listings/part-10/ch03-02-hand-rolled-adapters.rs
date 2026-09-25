// verify: debug ok
// Filter and Map written the way std writes them [LIB]: generic structs whose next() calls the inner next().
struct MyFilter<I, P> {
    iter: I,
    pred: P,
}

impl<I: Iterator, P: FnMut(&I::Item) -> bool> Iterator for MyFilter<I, P> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        // std: `self.iter.find(&mut self.predicate)`
        while let Some(x) = self.iter.next() {
            if (self.pred)(&x) {
                return Some(x);
            }
        }
        None
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, self.iter.size_hint().1) // might reject everything, can't add anything
    }
}

struct MyMap<I, F> {
    iter: I,
    f: F,
}

impl<B, I: Iterator, F: FnMut(I::Item) -> B> Iterator for MyMap<I, F> {
    type Item = B;
    fn next(&mut self) -> Option<B> {
        // std: `self.iter.next().map(&mut self.f)`
        self.iter.next().map(&mut self.f)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint() // one out per one in
    }
}

fn main() {
    let amounts: Vec<u64> = vec![120, 5, 980, 42, 3_000];
    let fee_bps = 290_u64;

    let mine: Vec<u64> = MyMap {
        iter: MyFilter { iter: amounts.iter(), pred: |a: &&u64| **a >= 100 },
        f: |a: &u64| a * fee_bps / 10_000,
    }
    .collect();
    let std_: Vec<u64> = amounts.iter().filter(|a| **a >= 100).map(|a| a * fee_bps / 10_000).collect();
    println!("hand-rolled: {mine:?}");
    println!("std:         {std_:?}");
    assert_eq!(mine, std_);
}
