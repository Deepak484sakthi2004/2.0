// verify: debug ok
/// Calls `f` with a line that THIS function creates and owns. `f` must work for any lifetime,
/// including one that ends inside this function: a higher-ranked bound.
fn for_each_line<F>(raw: &[u8], mut f: F) -> usize
where
    F: for<'line> FnMut(&'line str),
{
    let mut count = 0;
    for chunk in raw.split(|&b| b == b'\n') {
        let line = String::from_utf8_lossy(chunk).into_owned(); // owned by this iteration only
        f(&line);
        count += 1;
    } // `line` dropped here, every iteration
    count
}

fn main() {
    let raw = b"GET /health 200\nPOST /orders 201\nGET /orders 500";
    let mut errors = 0;
    let lines = for_each_line(raw, |line| {
        if line.ends_with("500") {
            errors += 1;
        }
    });
    println!("lines={lines} errors={errors}");
}
