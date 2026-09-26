// verify: debug crash overflowed its stack
// Listing 17.3-7: recursive descent has no depth limit of its own. Each `(` costs one pass through
// every precedence level (ten functions here, as in listing 17.3-1), so 50,000 nested parentheses
// overflow a thread's default 2 MiB stack, and the whole process aborts.

struct R<'a> { s: &'a [u8], i: usize }

impl R<'_> {
    fn peek(&self) -> u8 { self.s.get(self.i).copied().unwrap_or(0) }
    fn expr(&mut self) -> i64 { self.assign() }
    fn assign(&mut self) -> i64 { self.or() }
    fn or(&mut self) -> i64 {
        let mut v = self.and();
        while self.peek() == b'|' { self.i += 2; v |= self.and(); }
        v
    }
    fn and(&mut self) -> i64 {
        let mut v = self.cmp();
        while self.peek() == b'&' { self.i += 2; v &= self.cmp(); }
        v
    }
    fn cmp(&mut self) -> i64 {
        let v = self.add();
        if self.peek() == b'<' { self.i += 1; return (v < self.add()) as i64; }
        v
    }
    fn add(&mut self) -> i64 {
        let mut v = self.mul();
        while self.peek() == b'+' { self.i += 1; v += self.mul(); }
        v
    }
    fn mul(&mut self) -> i64 {
        let mut v = self.unary();
        while self.peek() == b'*' { self.i += 1; v *= self.unary(); }
        v
    }
    fn unary(&mut self) -> i64 {
        if self.peek() == b'-' { self.i += 1; return -self.unary(); }
        self.postfix()
    }
    fn postfix(&mut self) -> i64 { self.primary() }
    fn primary(&mut self) -> i64 {
        let c = self.peek();
        self.i += 1;
        if c == b'(' {
            let v = self.expr();
            self.i += 1; // ')'
            v
        } else {
            (c - b'0') as i64
        }
    }
}

fn main() {
    let depth = 50_000;
    let src = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
    println!("parsing {depth} nested parentheses on a thread with the default 2 MiB stack...");
    let handle = std::thread::spawn(move || R { s: src.as_bytes(), i: 0 }.expr());
    println!("value: {:?}", handle.join()); // never reached: the overflow aborts the process
}
