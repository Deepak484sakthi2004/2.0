// verify: debug ok
// verify: release ok
// Listing 17.3-8: two defenses against deep nesting.
//   1. Measure it: stack bytes per nesting level, ten-level recursive descent vs a Pratt loop.
//   2. Limit it: a depth counter that turns "abort the process" into "reject this input".

const MAX_DEPTH: usize = 256;

#[derive(Default)]
struct Probe { depth0: usize, depth_n: usize }

fn here() -> usize {
    let marker = 0u8;
    std::hint::black_box(&marker) as *const u8 as usize
}

// ---------- ten-level recursive descent (as in listing 17.3-7), with a depth limit ----------
struct Rd<'a> { s: &'a [u8], i: usize, depth: usize, probe_at: usize, probe: Probe }

impl Rd<'_> {
    fn peek(&self) -> u8 { self.s.get(self.i).copied().unwrap_or(0) }
    fn expr(&mut self) -> Result<i64, String> { self.assign() }
    fn assign(&mut self) -> Result<i64, String> { self.or() }
    fn or(&mut self) -> Result<i64, String> {
        let mut v = self.and()?;
        while self.peek() == b'|' { self.i += 2; v |= self.and()?; }
        Ok(v)
    }
    fn and(&mut self) -> Result<i64, String> {
        let mut v = self.cmp()?;
        while self.peek() == b'&' { self.i += 2; v &= self.cmp()?; }
        Ok(v)
    }
    fn cmp(&mut self) -> Result<i64, String> {
        let v = self.add()?;
        if self.peek() == b'<' { self.i += 1; return Ok((v < self.add()?) as i64); }
        Ok(v)
    }
    fn add(&mut self) -> Result<i64, String> {
        let mut v = self.mul()?;
        while self.peek() == b'+' { self.i += 1; v += self.mul()?; }
        Ok(v)
    }
    fn mul(&mut self) -> Result<i64, String> {
        let mut v = self.unary()?;
        while self.peek() == b'*' { self.i += 1; v *= self.unary()?; }
        Ok(v)
    }
    fn unary(&mut self) -> Result<i64, String> {
        if self.peek() == b'-' { self.i += 1; return Ok(-self.unary()?); }
        self.postfix()
    }
    fn postfix(&mut self) -> Result<i64, String> { self.primary() }
    fn primary(&mut self) -> Result<i64, String> {
        if self.depth == 0 { self.probe.depth0 = here(); }
        if self.depth == self.probe_at { self.probe.depth_n = here(); }
        let c = self.peek();
        self.i += 1;
        if c != b'(' {
            return Ok((c - b'0') as i64);
        }
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(format!("expression nested more than {MAX_DEPTH} levels deep at byte {}", self.i - 1));
        }
        let v = self.expr()?;
        self.depth -= 1;
        self.i += 1; // ')'
        Ok(v)
    }
}

// ---------- Pratt: one function per nesting level, plus the atom ----------
struct Pr<'a> { s: &'a [u8], i: usize, depth: usize, probe_at: usize, probe: Probe }

impl Pr<'_> {
    fn peek(&self) -> u8 { self.s.get(self.i).copied().unwrap_or(0) }
    fn infix(op: u8) -> Option<(u8, u8)> {
        match op { b'|' => Some((1, 2)), b'&' => Some((3, 4)), b'<' => Some((5, 6)),
                   b'+' => Some((7, 8)), b'*' => Some((9, 10)), _ => None }
    }
    fn expr_bp(&mut self, min_bp: u8) -> Result<i64, String> {
        let mut lhs = self.atom()?;
        while let Some((l, r)) = Self::infix(self.peek()) {
            if l < min_bp { break; }
            let op = self.peek();
            self.i += if op == b'|' || op == b'&' { 2 } else { 1 };
            let rhs = self.expr_bp(r)?;
            lhs = match op { b'|' => lhs | rhs, b'&' => lhs & rhs, b'<' => (lhs < rhs) as i64,
                             b'+' => lhs + rhs, _ => lhs * rhs };
        }
        Ok(lhs)
    }
    fn atom(&mut self) -> Result<i64, String> {
        if self.depth == 0 { self.probe.depth0 = here(); }
        if self.depth == self.probe_at { self.probe.depth_n = here(); }
        let c = self.peek();
        self.i += 1;
        match c {
            b'-' => Ok(-self.atom()?),
            b'(' => {
                self.depth += 1;
                if self.depth > MAX_DEPTH {
                    return Err(format!("expression nested more than {MAX_DEPTH} levels deep"));
                }
                let v = self.expr_bp(0)?;
                self.depth -= 1;
                self.i += 1;
                Ok(v)
            }
            _ => Ok((c - b'0') as i64),
        }
    }
}

fn nested(depth: usize) -> String {
    format!("{}1+2{}", "(".repeat(depth), ")".repeat(depth))
}

fn main() {
    let n = 200;
    let src = nested(n);
    let mut rd = Rd { s: src.as_bytes(), i: 0, depth: 0, probe_at: n, probe: Probe::default() };
    let v1 = rd.expr();
    let mut pr = Pr { s: src.as_bytes(), i: 0, depth: 0, probe_at: n, probe: Probe::default() };
    let v2 = pr.expr_bp(0);
    let per_rd = (rd.probe.depth0 - rd.probe.depth_n) / n;
    let per_pr = (pr.probe.depth0 - pr.probe.depth_n) / n;
    println!("{} build, {n} nested levels: value {v1:?} / {v2:?}", if cfg!(debug_assertions) { "debug" } else { "release" });
    println!("  stack per nesting level: recursive descent {per_rd} B, Pratt {per_pr} B");
    println!("  => a 2 MiB stack overflows near depth {} (RD) vs {} (Pratt)", (2 << 20) / per_rd.max(1), (2 << 20) / per_pr.max(1));

    for depth in [250, 257, 50_000] {
        let src = nested(depth);
        let r = Rd { s: src.as_bytes(), i: 0, depth: 0, probe_at: usize::MAX, probe: Probe::default() }.expr();
        println!("  depth {depth:>6}: {r:?}");
    }
}
