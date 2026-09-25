// verify: debug error:lifetime
trait Sink<T> {
    fn put(&mut self, item: T);
}

struct Collect<T>(Vec<T>);

impl<T> Sink<T> for Collect<T> {
    fn put(&mut self, item: T) {
        self.0.push(item);
    }
}

/// Allowed: the object's lifetime BOUND (`+ 'static` -> `+ 'a`) can shrink.
fn shorten_bound<'a>(s: Box<dyn Sink<u32> + 'static>) -> Box<dyn Sink<u32> + 'a> {
    s
}

/// Rejected: the trait's generic ARGUMENTS are invariant; &'static str -> &'a str is not allowed.
fn shorten_arg<'a>(s: Box<dyn Sink<&'static str>>) -> Box<dyn Sink<&'a str>> {
    s
}

fn main() {
    let mut s: Box<dyn Sink<&'static str>> = Box::new(Collect(Vec::new()));
    s.put("static text");
    let _shorter = shorten_arg(s);
    let _bounded = shorten_bound(Box::new(Collect(Vec::new())));
}
