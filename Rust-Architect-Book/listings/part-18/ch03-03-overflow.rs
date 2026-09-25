// verify: debug error:E0275
// A blanket impl whose where-clause asks about a BIGGER type. Proving `Vec<u64>: Wire` needs
// `Vec<Box<u64>>: Wire`, which needs `Vec<Box<Box<u64>>>: Wire`, and so on: the solver gives up
// at the recursion limit.
trait Wire {
    fn size(&self) -> usize;
}

impl Wire for u64 {
    fn size(&self) -> usize {
        8
    }
}

impl<T> Wire for Vec<T>
where
    Vec<Box<T>>: Wire,
{
    fn size(&self) -> usize {
        self.len() * 8
    }
}

fn send<W: Wire>(w: &W) -> usize {
    w.size()
}

fn main() {
    println!("{}", send(&vec![1u64, 2, 3]));
}
