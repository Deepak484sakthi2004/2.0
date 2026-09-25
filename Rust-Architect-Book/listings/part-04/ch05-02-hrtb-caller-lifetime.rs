// verify: debug error:E0597
/// Wrong bound: 'line is chosen by the CALLER, so it must outlive this whole call,
/// and a line created inside the function can never satisfy it.
fn for_each_line<'line, F>(raw: &[u8], mut f: F) -> usize
where
    F: FnMut(&'line str),
{
    let mut count = 0;
    for chunk in raw.split(|&b| b == b'\n') {
        let line = String::from_utf8_lossy(chunk).into_owned();
        f(&line);
        count += 1;
    }
    count
}

fn main() {
    let raw = b"GET /health 200\nPOST /orders 201";
    println!("{}", for_each_line(raw, |line| println!("{line}")));
}
