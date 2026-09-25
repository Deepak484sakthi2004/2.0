// verify: debug error:E0505
fn consume(v: Vec<u32>) -> usize {
    v.len()
}

fn main() {
    let v = vec![1, 2, 3];
    let first = &v[0];
    let n = consume(v);
    println!("{first} {n}");
}
