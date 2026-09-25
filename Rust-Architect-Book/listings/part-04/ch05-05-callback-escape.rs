// verify: debug error:E0521
fn for_each_line<F>(raw: &[u8], mut f: F)
where
    F: for<'line> FnMut(&'line str),
{
    for chunk in raw.split(|&b| b == b'\n') {
        let line = String::from_utf8_lossy(chunk).into_owned();
        f(&line);
    }
}

fn main() {
    let raw = b"GET /health 200\nGET /orders 500";
    let mut failures: Vec<&str> = Vec::new();
    for_each_line(raw, |line| {
        if line.ends_with("500") {
            failures.push(line); // try to KEEP a line after the callback returns
        }
    });
    println!("{failures:?}");
}
