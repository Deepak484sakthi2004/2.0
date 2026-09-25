// verify: debug ok
/// A zero-copy tokenizer. Tokens borrow from the INPUT ('a), not from the tokenizer.
struct Tokenizer<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Tokenizer<'a> {
    fn new(input: &'a str) -> Self {
        Tokenizer { input, pos: 0 }
    }

    /// `&mut self` is borrowed only for the call; the returned token lives as long as the input.
    fn next_token(&mut self) -> Option<&'a str> {
        let rest = &self.input[self.pos..];
        let start = rest.len() - rest.trim_start().len();
        let rest = &rest[start..];
        if rest.is_empty() {
            return None;
        }
        let len = rest.find(char::is_whitespace).unwrap_or(rest.len());
        self.pos += start + len;
        Some(&rest[..len])
    }
}

fn main() {
    let line = String::from("GET /api/orders HTTP/1.1");
    let mut tokens = Tokenizer::new(&line);
    let method = tokens.next_token(); // holds a token...
    let path = tokens.next_token(); // ...while calling next_token again: fine
    let version = tokens.next_token();
    println!("{method:?} {path:?} {version:?} {:?}", tokens.next_token());
}
