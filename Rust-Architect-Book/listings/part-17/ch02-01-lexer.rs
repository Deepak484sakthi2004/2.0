// verify: debug ok
// verify: debug test
// Listing 17.2-1: the Ore lexer. Hand-written, byte-oriented, zero-copy for identifiers,
// maximal munch for operators, spans on every token, and error tokens instead of aborting.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub lo: u32, // byte offset of the first byte
    pub hi: u32, // byte offset one past the last byte
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokKind {
    Int(i64),
    Str(String), // escapes already processed
    Ident,       // the text is &src[span]: no allocation
    // keywords
    Fn, Let, Mut, If, Else, While, Return, True, False,
    // punctuation and operators
    LParen, RParen, LBrace, RBrace, Comma, Semi, Colon, Arrow,
    Plus, Minus, Star, Slash, Percent, Assign, EqEq, Ne, Lt, Le, Gt, Ge, AndAnd, OrOr, Bang,
    Error(String),
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokKind,
    pub span: Span,
}

pub struct Lexer<'a> {
    src: &'a str,
    b: &'a [u8],
    pos: usize,
    done: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Lexer { src, b: src.as_bytes(), pos: 0, done: false }
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.b.get(self.pos) {
                Some(c) if c.is_ascii_whitespace() => self.pos += 1,
                Some(b'/') if self.b.get(self.pos + 1) == Some(&b'/') => {
                    while self.pos < self.b.len() && self.b[self.pos] != b'\n' {
                        self.pos += 1;
                    }
                }
                _ => return,
            }
        }
    }

    fn number(&mut self) -> TokKind {
        let (mut v, mut overflow) = (0i64, false);
        while let Some(&d) = self.b.get(self.pos) {
            match d {
                b'0'..=b'9' => match v.checked_mul(10).and_then(|v| v.checked_add((d - b'0') as i64)) {
                    Some(n) => v = n,
                    None => overflow = true,
                },
                b'_' => {}
                _ => break,
            }
            self.pos += 1;
        }
        if overflow {
            TokKind::Error("integer literal is too large for `int` (i64)".into())
        } else {
            TokKind::Int(v)
        }
    }

    fn ident_or_keyword(&mut self) -> TokKind {
        let start = self.pos;
        while let Some(c) = self.b.get(self.pos) {
            if c.is_ascii_alphanumeric() || *c == b'_' {
                self.pos += 1;
            } else {
                break;
            }
        }
        match &self.src[start..self.pos] {
            "fn" => TokKind::Fn,
            "let" => TokKind::Let,
            "mut" => TokKind::Mut,
            "if" => TokKind::If,
            "else" => TokKind::Else,
            "while" => TokKind::While,
            "return" => TokKind::Return,
            "true" => TokKind::True,
            "false" => TokKind::False,
            _ => TokKind::Ident, // note: `int` and `bool` are ordinary names, resolved later
        }
    }

    fn string(&mut self) -> TokKind {
        self.pos += 1; // the opening quote
        let (mut out, mut bad_escape) = (String::new(), None);
        let mut chars = self.src[self.pos..].char_indices();
        while let Some((off, ch)) = chars.next() {
            match ch {
                '"' => {
                    self.pos += off + 1;
                    return match bad_escape {
                        Some(e) => TokKind::Error(format!("unknown escape `\\{e}`")),
                        None => TokKind::Str(out),
                    };
                }
                // Stop at the end of the line: the next line still lexes normally (error recovery).
                '\n' => {
                    self.pos += off;
                    return TokKind::Error("unterminated string literal".into());
                }
                '\\' => match chars.next() {
                    Some((_, 'n')) => out.push('\n'),
                    Some((_, 't')) => out.push('\t'),
                    Some((_, '\\')) => out.push('\\'),
                    Some((_, '"')) => out.push('"'),
                    Some((_, other)) => bad_escape = bad_escape.or(Some(other)),
                    None => break,
                },
                c => out.push(c),
            }
        }
        self.pos = self.src.len();
        TokKind::Error("unterminated string literal".into())
    }

    fn punct(&mut self) -> TokKind {
        use TokKind::*;
        let c = self.b[self.pos];
        // Maximal munch: check the two-byte operators before their one-byte prefixes.
        let (kind, len) = match (c, self.b.get(self.pos + 1).copied()) {
            (b'-', Some(b'>')) => (Arrow, 2),
            (b'=', Some(b'=')) => (EqEq, 2),
            (b'!', Some(b'=')) => (Ne, 2),
            (b'<', Some(b'=')) => (Le, 2),
            (b'>', Some(b'=')) => (Ge, 2),
            (b'&', Some(b'&')) => (AndAnd, 2),
            (b'|', Some(b'|')) => (OrOr, 2),
            (b'(', _) => (LParen, 1),
            (b')', _) => (RParen, 1),
            (b'{', _) => (LBrace, 1),
            (b'}', _) => (RBrace, 1),
            (b',', _) => (Comma, 1),
            (b';', _) => (Semi, 1),
            (b':', _) => (Colon, 1),
            (b'+', _) => (Plus, 1),
            (b'-', _) => (Minus, 1),
            (b'*', _) => (Star, 1),
            (b'/', _) => (Slash, 1),
            (b'%', _) => (Percent, 1),
            (b'=', _) => (Assign, 1),
            (b'<', _) => (Lt, 1),
            (b'>', _) => (Gt, 1),
            (b'!', _) => (Bang, 1),
            _ => {
                // Skip one whole UTF-8 character, so that lexing can continue after it.
                let ch = self.src[self.pos..].chars().next().unwrap();
                let msg = if ch.is_alphabetic() {
                    format!("non-ASCII identifier character {ch:?} (U+{:04X}); Ore identifiers are ASCII", ch as u32)
                } else {
                    format!("unexpected character {ch:?} (U+{:04X})", ch as u32)
                };
                (Error(msg), ch.len_utf8())
            }
        };
        self.pos += len;
        kind
    }
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    /// Yields tokens lazily (pull-based), ending with exactly one `Eof`.
    fn next(&mut self) -> Option<Token> {
        if self.done {
            return None;
        }
        self.skip_trivia();
        let start = self.pos;
        let kind = match self.b.get(self.pos) {
            None => {
                self.done = true;
                TokKind::Eof
            }
            Some(b'0'..=b'9') => self.number(),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'_') => self.ident_or_keyword(),
            Some(b'"') => self.string(),
            Some(_) => self.punct(),
        };
        Some(Token { kind, span: Span { lo: start as u32, hi: self.pos as u32 } })
    }
}

/// Line starts, computed once; spans are converted to line:column only when a message is printed.
pub struct LineIndex {
    starts: Vec<u32>,
}

impl LineIndex {
    pub fn new(src: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(src.bytes().enumerate().filter(|&(_, b)| b == b'\n').map(|(i, _)| i as u32 + 1));
        LineIndex { starts }
    }
    pub fn line_col(&self, pos: u32) -> (u32, u32) {
        let line = self.starts.partition_point(|&s| s <= pos) - 1;
        (line as u32 + 1, pos - self.starts[line] + 1) // 1-based; column counted in bytes
    }
}

fn main() {
    let src = "// Ore: a first program\nfn sum_to(n: int) -> int {\n    let mut total = 0;\n    while n > 0 { total = total + n; n = n - 1; }\n    total\n}\n";
    let lines = LineIndex::new(src);
    let toks: Vec<Token> = Lexer::new(src).collect();
    println!("{} tokens", toks.len());
    for t in toks.iter().take(14) {
        let (l, c) = lines.line_col(t.span.lo);
        let text = &src[t.span.lo as usize..t.span.hi as usize];
        println!("  {l}:{c:<3} {:<10} {text:?}", format!("{:?}", t.kind));
    }

    println!("\nerror recovery: every error becomes a token, and lexing goes on");
    let bad = "let s = \"unterminated\nlet \u{e9}t\u{e9} = 99999999999999999999;\nlet t = 1 \u{2295} 2; let u = \"a\\qb\";";
    let lines = LineIndex::new(bad);
    for t in Lexer::new(bad) {
        if let TokKind::Error(msg) = &t.kind {
            let (l, c) = lines.line_col(t.span.lo);
            println!("  {l}:{c}: error: {msg}");
        }
    }
    let count = Lexer::new(bad).filter(|t| !matches!(t.kind, TokKind::Error(_))).count();
    println!("  ...and {count} good tokens around them");
}

#[cfg(test)]
mod tests {
    use super::TokKind::*;
    use super::*;

    fn kinds(src: &str) -> Vec<TokKind> {
        Lexer::new(src).map(|t| t.kind).collect()
    }

    #[test]
    fn maximal_munch() {
        assert_eq!(kinds("a<=b->c==d"), vec![Ident, Le, Ident, Arrow, Ident, EqEq, Ident, Eof]);
        assert_eq!(kinds("a< =b"), vec![Ident, Lt, Assign, Ident, Eof]); // whitespace splits tokens
        assert_eq!(kinds("x--1"), vec![Ident, Minus, Minus, Int(1), Eof]); // no `--` operator in Ore
    }

    #[test]
    fn keywords_are_whole_words() {
        assert_eq!(kinds("if iffy fn fnord"), vec![If, Ident, Fn, Ident, Eof]);
    }

    #[test]
    fn literals() {
        assert_eq!(kinds("1_000_000"), vec![Int(1_000_000), Eof]);
        assert_eq!(kinds("9223372036854775807"), vec![Int(i64::MAX), Eof]);
        assert!(matches!(kinds("9223372036854775808")[0], Error(_))); // i64::MIN needs `-` + a too-big literal
        assert_eq!(kinds(r#""a\"b\n""#), vec![Str("a\"b\n".into()), Eof]);
    }

    #[test]
    fn comments_and_spans() {
        let toks: Vec<Token> = Lexer::new("x // ignored\n  y").collect();
        assert_eq!(toks.len(), 3);
        assert_eq!(toks[1].span, Span { lo: 15, hi: 16 });
        assert_eq!(LineIndex::new("x // ignored\n  y").line_col(15), (2, 3));
    }

    #[test]
    fn errors_do_not_stop_the_lexer() {
        let k = kinds("a # b");
        assert_eq!(k.len(), 4);
        assert!(matches!(k[1], Error(_)));
        assert_eq!(k[2], Ident);
    }
}
