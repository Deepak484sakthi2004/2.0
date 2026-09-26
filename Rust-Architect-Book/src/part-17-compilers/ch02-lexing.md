# Chapter 17.2 — Lexing

> **Where this sits:** Part XVII · Compilers · chapter 2 of 8
> **Prerequisites:** Chapter 17.1 (the pipeline); Chapter 3.4 (UTF-8: bytes, chars, and why indexing a `str` is
> subtle); Chapter 9.2 (byte-class tables and `memchr`).
> **After this chapter you can:** write a byte-oriented lexer with spans and error tokens; explain maximal munch and
> why every lexer is a finite automaton; choose between zero-copy and owned tokens using measured costs; and defend a
> language against Unicode look-alikes and invisible direction controls.

---

## Pass 1 · User level — *Turning bytes into words*

### 1. Problem

The lexer is the only stage of a compiler that looks at every byte. Everything after it sees tokens: `Ident`,
`Int(42)`, `LtEq`. That makes the lexer the place where four decisions are made, usually without anyone noticing:

- **Where tokens start and end.** Is `a<=b` three tokens or four? Is `iffy` a keyword followed by `fy`? Is `x--1` an
  error, a decrement, or `x - (-1)`?
- **What text means.** Is `1_000_000` an integer? Is `99999999999999999999` an integer, an error, or silently
  something else? Does a string literal's `\q` mean anything?
- **Where each token came from.** Every error message later in the pipeline points at a *span* that the lexer
  recorded. A lexer that forgets positions forces every later error to say "parse error" with no location.
- **What text is allowed to look like.** Two strings that print identically can be different byte sequences. The
  lexer is the first and cheapest place to notice.

Most DSLs at work skip this stage: they call `split_whitespace()` and match on words. That works until someone writes
`amount>1000` without spaces, or pastes a rule from a PDF. This chapter builds Ore's real lexer, measures what tokens
cost, shows that the lexer is a finite automaton in disguise, and ends with the incident that made Meridian's Sieve
lexer reject non-ASCII characters in literals.

### 2. Mental model

A lexer is a function from a byte string to a sequence of `(kind, span)` pairs, governed by three rules:

```text
source:  while n > 0 { total = total + n; }
          ^^^^^ ^ ^ ^ ^ ^^^^^ ^ ^^^^^ ^ ^^
tokens:  While Ident Gt Int(0) LBrace Ident Assign Ident Plus Ident Semi RBrace Eof
spans:   0..5  6..7  8..9 10..11 12..13 14..19 20..21 22..27 28..29 30..31 31..32 33..34 34..34
```

1. **Maximal munch.** At each position, take the *longest* token that matches. `<=` is one token, not `<` then `=`;
   `iffy` is one identifier, not the keyword `if` then `fy`. Keywords are recognized by lexing a whole identifier and
   *then* looking it up, which is why "keywords are whole words" is automatic.
2. **Tokens carry spans, not text.** A token is a kind plus a byte range `lo..hi` into the source. The text is
   `&src[lo..hi]` whenever someone needs it, and line and column are computed only when an error message is printed.
3. **Errors are tokens.** An unexpected character becomes an `Error` token, and lexing continues after it. One stray
   byte shouldn't hide the next ten mistakes, and the parser can decide how to recover.

Where does this sit in the theory? Tokens are described by *regular* languages (identifiers, numbers, operators), and
every regular language is recognized by a deterministic finite automaton (DFA): a fixed table of states and
transitions, no stack. Nesting (parentheses inside parentheses) is not regular, so it's the *parser's* job
(Chapter 17.3). That split, regular tokens below and context-free structure above, is why compilers have two front-end
stages instead of one.

### 3. Rust code

Listing `ch02-01-lexer.rs` is Ore's lexer: byte-oriented, zero-copy for identifiers, with spans on every token and
error tokens instead of aborting. A token is small and owns nothing for identifiers:

```rust,ignore
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
```

(Excerpt of listing 17.2-1; the full file is verified.) Maximal munch for operators is one `match` on the current byte
*and the next one*, with the two-byte forms listed first:

```rust,ignore
        let (kind, len) = match (c, self.b.get(self.pos + 1).copied()) {
            (b'-', Some(b'>')) => (Arrow, 2),
            (b'=', Some(b'=')) => (EqEq, 2),
            (b'!', Some(b'=')) => (Ne, 2),
            (b'<', Some(b'=')) => (Le, 2),
            (b'>', Some(b'=')) => (Ge, 2),
            (b'&', Some(b'&')) => (AndAnd, 2),
            (b'|', Some(b'|')) => (OrOr, 2),
            (b'(', _) => (LParen, 1),
            // ... one-byte operators ...
            _ => {
                // Skip one whole UTF-8 character, so that lexing can continue after it.
                let ch = self.src[self.pos..].chars().next().unwrap();
                // ... build the message: "unexpected character" or "non-ASCII identifier character" ...
                (Error(msg), ch.len_utf8())
            }
        };
```

(Excerpt of listing 17.2-1, `Lexer::punct`, lightly elided.) Rust's `match` tries arms in order, so the order *is* the
maximal-munch rule. Notice the error arm: it skips a whole `char`, not a byte, so a multi-byte character produces one
error and the next token starts on a character boundary (Chapter 3.4's rule, applied).

The lexer is an `Iterator`: the parser *pulls* tokens one at a time, and the stream ends with exactly one `Eof`.
Positions are byte offsets; a `LineIndex` built once (the offsets of every `\n`) turns an offset into `line:column`
with a binary search, and only when a message is printed. Running the lexer on a small Ore function (real output,
first 14 tokens):

```text
37 tokens
  2:1   Fn         "fn"
  2:4   Ident      "sum_to"
  2:10  LParen     "("
  2:11  Ident      "n"
  2:12  Colon      ":"
  2:14  Ident      "int"
  2:17  RParen     ")"
  2:19  Arrow      "->"
  2:22  Ident      "int"
  2:26  LBrace     "{"
  3:5   Let        "let"
  3:9   Mut        "mut"
  3:13  Ident      "total"
  3:19  Assign     "="
```

The comment on line 1 produced no tokens: comments and whitespace are *trivia*, skipped before each token. Note also
that `int` is an `Ident`, not a keyword. Type names are ordinary names that the resolver looks up later (Chapter
17.4), which keeps the lexer small and lets a program shadow them.

Now a deliberately broken input: an unterminated string, a non-ASCII identifier, an integer that doesn't fit in
`i64`, an unknown operator, and a bad escape. Every error becomes a token, and lexing continues (real output):

```text
error recovery: every error becomes a token, and lexing goes on
  1:9: error: unterminated string literal
  2:5: error: non-ASCII identifier character 'é' (U+00E9); Ore identifiers are ASCII
  2:8: error: non-ASCII identifier character 'é' (U+00E9); Ore identifiers are ASCII
  2:13: error: integer literal is too large for `int` (i64)
  3:11: error: unexpected character '⊕' (U+2295)
  3:26: error: unknown escape `\q`
  ...and 18 good tokens around them
```

Two recovery decisions are visible here. The unterminated string stops at the end of *its line*, so line 2 still
lexes normally; a lexer that ran to the end of the file would have swallowed the rest of the program into one bad
token. And the integer overflow is detected with `checked_mul`/`checked_add` while the digits are accumulated, then
reported with the literal's span. (Ore's `i64::MIN` needs `-` applied to a literal that is itself too big, the same
corner Rust handles specially.) The listing's five unit tests pin the rules down, including maximal munch
(`a<=b->c==d`), whitespace splitting tokens (`a< =b` is `Lt Assign`), and keywords as whole words (`if iffy fn fnord`),
and all five pass.

---

## Pass 2 · Systems level — *A lexer is a table*

### 4. Under the hood

**Every lexer is a DFA.** Listing `ch02-06-dfa-lexer.rs` makes that literal. It builds two tables at compile time
(`const fn`): a 256-entry byte → character-class table, and a (state, class) → state transition table with 12 states
and 11 classes. The lexing loop runs the automaton from the start state, remembers the *last accepting state* it
passed through, and when the automaton hits a dead state it emits the longest match it saw. That "remember the last
accept" loop is maximal munch, stated as an algorithm:

```rust,ignore
        let (mut state, mut j, mut last) = (START, i, None);
        while j < src.len() {
            let next = TABLE[state as usize][CLASSES[src[j] as usize] as usize];
            if next == DEAD {
                break;
            }
            state = next;
            j += 1;
            if let Some(k) = ACCEPT[state as usize] {
                last = Some((k, j)); // longest match so far
            }
        }
```

(Excerpt of listing 17.2-6.) Real output:

```text
tables: 256 bytes of classes + 12x11 transitions = 388 bytes
fn:Ident  f:Ident  (:Op  a1:Ident  ::Op  int:Ident  ):Op  ->:Op  int:Ident  {:Op  a1:Ident  <=:Op  b:Ident  ->:Op  c:Ident  &&:Op  d:Ident  ||:Op  !:Op  e:Ident  ==:Op  42:Int  }:Op  x:Ident  &:Error  y:Ident
DFA and hand-written lexers agree on 2,000 random inputs
```

The whole lexer is 388 bytes of tables plus a ten-line loop. A lone `&` is an error because the state after `&` isn't
accepting: Ore has `&&` but no `&`. The last line is a **differential test**: 2,000 random 40-byte inputs over an
alphabet chosen to hit every operator prefix, and the table-driven and hand-written lexers agree on every one. That
is the cheapest way to test a lexer rewrite, and it's the same idea as Meridian's shadow mode in Chapter 17.3.

This table shape is what lexer generators emit: `lex`/`flex` for C, `re2c`, and Rust's `logos` crate (a derive macro
that compiles token regexes into a state machine at build time) [LIB]. `logos` isn't available on the Playground, so
it isn't used here; listing 17.2-6 is the hand-built equivalent of its output.

**rustc's lexer** [RUSTC]. rustc lexes in two layers. The `rustc_lexer` crate is a small, standalone, hand-written
lexer that walks the `char`s of a `&str` and produces `(kind, length)` pairs: no spans, no interning, no errors
reported, just classification. It's deliberately reusable, and rust-analyzer consumes it too (through a published copy
of the crate) [LIB]. The parser crate (`rustc_parse`) wraps it: it converts lengths into absolute spans,
interns identifiers into `Symbol`s (Chapter 17.4), validates and unescapes literals, reports lexical errors, and
groups tokens into *token trees* (delimited groups) because macros consume token trees rather than raw tokens. Those
details change between releases; the division of labor (a pure classifier underneath, diagnostics and structure on
top) is the stable idea, and it's the same division as `lex` versus `LineIndex` and error tokens in listing 17.2-1.

**Identifiers and Unicode** [LANG]. Rust allows non-ASCII identifiers (RFC 2457, stable since 1.53) using Unicode's
identifier rules (UAX #31: `XID_Start` followed by `XID_Continue`), and the Reference specifies that identifiers are
normalized to NFC, so two spellings of `é` (one precomposed code point, or `e` plus a combining accent) name the same
thing. Ore chose the opposite policy, ASCII identifiers only, which is a legitimate language-design choice for a
language whose names come from a catalog.

### 5. Memory

The measurable question is what a token *costs*. Listing `ch02-02-lexer-cost.rs` lexes the same 2 MB of synthetic Ore
three ways, with the counting allocator from Part III installed (allocations are exact):

```text
input 2.00 MB, 442872 tokens (196832 identifiers, 233738 tokens with text)
allocations:
  A  spans, streamed                    0
  A' spans, collected into a Vec       18   (Vec growth only)
  B  owned String per lexeme       233756
  C  chars().peekable()            516192
```

- **A, zero-copy.** The scanner calls back with `(kind, lo, hi)`. Nothing is allocated, ever. Collecting the
  `(kind, u32, u32)` triples into a `Vec` costs only the vector's doubling: 18 allocations for 442,872 tokens.
- **B, owned text.** Each identifier and number token gets its own `String`: 233,738 allocations for the lexemes plus
  the vector's 18.
- **C, `chars().peekable()`.** The common first attempt: build every lexeme `char` by `char` into a fresh `String`,
  operators included. That's one allocation per token (442,872) plus reallocations when a lexeme outgrows its
  buffer, for 516,192 in total.

The zero-copy design has a price, and it's a lifetime. A token that holds `&'src str`, or a span that is only
meaningful next to the source it indexes, ties every later structure to the source buffer (Part IV). Compilers pay that
price in one of two ways. Either they keep the source alive for the whole compilation (rustc's `SourceMap` holds every
file for the session) [RUSTC], or they intern identifiers into a table and let tokens carry 4-byte symbols instead of
text (Chapter 17.4). Ore's lexer uses both: spans for everything, and interning later.

Spans themselves are a layout decision. Listing 17.2-1 uses two `u32`s (8 bytes), which caps a file at 4 GiB and is
half the size of a `&str` (16 bytes, a fat pointer). rustc's `Span` is also 8 bytes: a compressed encoding of a
32-bit position, a length, and a hygiene context, with a side table ("span interner") for spans that don't fit
[RUSTC]. Line and column are not stored per token in either design; they're derived on demand from a table of line
starts.

### 6. CPU / OS

The same listing times the three lexers (release, best of 5, one Playground run: noisy):

```text
throughput (release, best of 5, one run):
  A  spans   1264.7 MB/s     3.57 ns/token
  B  owned     92.9 MB/s    48.61 ns/token
  C  chars     60.3 MB/s    74.95 ns/token
```

Allocation dominates. The owned-text lexer does the *same scanning* as the zero-copy one and is about 13.6× slower,
because each token now costs a trip through the allocator and a copy. The `chars()` version adds UTF-8 decoding of
every character and a `String` per operator. At about 3.6 ns per token, the zero-copy lexer is spending most of its time
on the branches that classify bytes.

Two CPU-level refinements follow from Chapter 9.2:

- **A class table replaces a comparison chain.** `CLASSES[byte]` in listing 17.2-6 is one load from a 256-byte table
  that stays in L1 cache, instead of up to a dozen compare-and-branch pairs. Branch mispredictions are what make
  lexers slow on real code, where token kinds alternate unpredictably.
- **SIMD search helps only for long, boring stretches.** Skipping to the end of a comment or a string literal is a
  search for one or two bytes (`\n`, `"`, `\\`), which `memchr` does 16 or 32 bytes at a time. Ordinary code, where a
  new token starts every few bytes, doesn't benefit (the same conclusion Chapter 9.2 reached for `redact`).

At the OS level, lexing is where the compiler meets the file system. rustc reads each source file completely into
memory and validates it as UTF-8 before lexing [RUSTC]. An IDE lexer does something different: it re-lexes only the
edited region, which is one reason rust-analyzer wants a lexer that is a pure function from text to tokens.

---

## Pass 3 · Architect level — *The first line of defense*

### 7. Trade-offs

| Approach | Control over errors | Performance | Effort | Typical use |
|---|---|---|---|---|
| `split_whitespace()` + `match` | none: positions lost | fine | minutes | prototypes that become production by accident |
| Regular expressions (a `RegexSet`, or one big alternation) | weak: longest match across alternatives needs care | good | hours | log formats, simple DSLs |
| Generated DFA (`logos`, `re2c`, `flex`) | medium: error tokens are possible | best | hours, plus a build dependency | languages with many token kinds |
| Hand-written (listing 17.2-1) | full: every message is yours | very good | a day | compilers (rustc, javac, Go, Clang) |
| Parser combinators doing the lexing too (`nom`) | medium | good | hours | binary formats, small grammars |

Production compilers overwhelmingly hand-write their lexers. The reason isn't speed; a generated DFA is at least as
fast. It's diagnostics: a hand-written lexer can say "unterminated string literal" at the right place, suggest the
escape you meant, and recover sensibly, and those messages are most of a language's user experience at the lexical
level.

Three smaller decisions recur in every lexer:

- **Pull, push, or collect.** An `Iterator` (pull) lets the parser drive and stop early. A callback (push, listing
  17.2-2's `lex_spans`) is the fastest. Collecting everything into a `Vec` first makes arbitrary lookahead trivial,
  and it's what rustc effectively does with token trees.
- **What goes in the token.** Literal *values* (`Int(i64)`) or just their spans. Computing values in the lexer puts
  overflow errors in the right place; deferring them keeps tokens smaller.
- **What the lexer rejects versus what the parser rejects.** `x--1` is three valid tokens in Ore, and whether it means
  anything is the parser's decision. A lexer that tries to understand structure becomes a bad parser.

> **Why not just use regexes?** You can, and for a log format you should. For a language, two things go wrong. A
> regex alternation picks the *first* alternative that matches, not the longest, so `<` must be listed after `<=`
> everywhere, forever. And "what went wrong, where" comes back as "no match", which is the error message users see.
> `logos` solves the first problem by compiling all token patterns into one DFA, which is listing 17.2-6's design.

### 8. Java comparison

`javac` has a hand-written scanner (`JavaTokenizer` in the OpenJDK sources) [LIB], and its lexical rules hold a
surprise that Rust's don't: **Unicode escapes are translated before lexing** (JLS §3.3). `"` *is* a double quote to
the Java lexer, anywhere in the file, including inside comments. The classic consequence is that `// \u000a` ends
the comment, because `\u000a` becomes a newline before the lexer decides where the comment ends. Rust has no such
pre-pass: `\u{...}` escapes exist only inside character and string literals, where the lexer processes them as part
of the literal.

Java identifiers may use any Unicode letter (`Character.isJavaIdentifierStart`), and javac doesn't warn about
confusable identifiers. It interns identifier text into a name table (`Name`) so later phases compare names cheaply,
which is the idea Chapter 17.4 implements [LIB]. For DSLs, the Java ecosystem's default is a generator: ANTLR produces
a lexer and parser from one grammar file, and its generated lexer does longest-match tokenization, the job listing
17.2-6 does by hand.

| | Java (javac) | Rust (rustc) |
|---|---|---|
| Source unit | UTF-16 `char`s, after `\uXXXX` translation | UTF-8 text (`&str`), no escape pre-pass |
| Identifiers | Unicode letters (`isJavaIdentifierStart`) | UAX #31, NFC-normalized, confusable lints |
| Lexer | hand-written `JavaTokenizer` | hand-written `rustc_lexer` + `rustc_parse` wrapper |
| Bidi controls in literals | no javac check verified here | rejected by a deny-by-default lint (since 1.56.1) |

> **Analogy limit.** "javac's scanner and rustc's lexer do the same job" is true at this level of detail, but the
> units differ. A Java `char` is a UTF-16 code unit, so a character outside the Basic Multilingual Plane is *two*
> `char`s to the Java lexer, while rustc works with Unicode scalar values (`char`) over UTF-8. Column numbers in error
> messages are computed differently as a result, which matters when tools map positions between languages.

### 9. Production scenario

**Sieve's lexer.** Sieve (Chapter 17.1) is compiled in the rule service when an analyst saves a rule, and its lexer
encodes three decisions the risk platform team made on purpose:

- **Spans everywhere, errors as tokens.** The rule editor shows every error in a rule at once, with squiggles at the
  right characters. That requires the listing 17.2-1 design: a span on every token, error tokens instead of aborting,
  and a line index for converting offsets into editor positions.
- **Money is a token kind.** `EUR 1000.00` lexes as one `Money` token: a currency code followed by a decimal literal,
  converted *exactly* into minor units (100000) at lex time. The decimal text is never parsed through `f64`, for the
  reason Chapter 2.3 gave (an `f64` holds integers exactly only up to 2^53, and decimal fractions like 0.1 not at all).
  Too many decimal places for the currency is a lexical error with a span.
- **Names come from a catalog, so identifiers are ASCII.** Every feature a rule can read (`amount`, `country`,
  `velocity_1h`) is defined in the feature catalog as ASCII snake_case, so a non-ASCII identifier can only be a
  mistake. String literals are ASCII too: a rule that needs a non-ASCII value spells it with an escape
  (`"M\u{fc}nchen"`), which is visible in code review. §10 is why.

The lexer is about 300 lines, is fuzzed on every commit with random byte strings (the only property checked is "never
panics, every byte is covered by exactly one token"), and has a differential test against a table-driven version,
like listing 17.2-6.

### 10. Failure scenario

**The country code that wasn't.** During Sieve's beta in April 2026, the fraud team moved a manual-review rule for
high-risk German merchant categories into Sieve. The analyst copied the country code from a regulator's PDF, and the
PDF's text layer contained a Cyrillic capital letter IE (U+0415) that renders exactly like a Latin `E`. The rule
`country == "DЕ"` compiled, deployed, and never matched. Listing `ch02-03-confusable-literal.rs` reproduces it (real
output):

```text
rule   country == "DE"
  field country, literal "DE": 2 chars, bytes [44 45]
  matches country="DE"? true
  lint: clean
rule   country == "DЕ"
  field country, literal "DЕ": 2 chars, bytes [44 D0 95]
  matches country="DE"? false
  lint: warning: byte 1: U+0415 CYRILLIC CAPITAL LETTER IE looks like 'E' (mixed with ASCII letters)
```

The two rules print identically. Only the bytes differ: the second literal is three bytes, because `Е` needs two in
UTF-8. Nothing crashed and nothing errored; a rule that never fires simply looks like a quiet rule. It ran for 16 days
before the platform's weekly "rules that haven't fired in 7 days" report listed it, and a reviewer compared bytes.
The manual-review queue for those merchant categories had been empty the whole time.

The fixes went into the lexer, because that's the first stage that sees the characters:

1. **ASCII-only literals by default** (§9), with non-ASCII spelled as escapes.
2. **A confusable check** for the cases where non-ASCII is allowed: listing 17.2-3's `lint_literal` flags characters
   from a look-alike table and says which Latin letter each resembles. Real tools use Unicode's confusables data
   (UTS #39, "Unicode Security Mechanisms"), which lists thousands of pairs; the listing's seven-entry table shows the
   mechanism.
3. **An alert on "never fired"** as a first-class rule-health signal, because some wrong rules can't be detected
   statically at all.

**What rustc does** [LANG] [RUSTC]. Rust faces the same problem in identifiers, and ships lints for it. Listing
`ch02-04-rust-confusable-idents.rs` declares a Latin `e` and a Cyrillic `е` in the same function, with the confusable
lints denied (both are warn-by-default):

```rust,compile_fail
#![deny(confusable_idents, mixed_script_confusables)]

fn main() {
    let e = 1; // Latin small e
    let е = 2; // Cyrillic small ie (U+0435)
    println!("{}", e + е);
}
```

```text
error: found both `e` and `е` as identifiers, which look alike
 --> src/main.rs:8:9
  |
7 |     let e = 1; // Latin small e
  |         - other identifier used here
8 |     let е = 2; // Cyrillic small ie (U+0435)
  |         ^ this identifier can be confused with `e`
...
error: the usage of Script Group `Cyrillic` in this crate consists solely of mixed script confusables
 --> src/main.rs:8:9
  |
8 |     let е = 2; // Cyrillic small ie (U+0435)
  |         ^
  |
  = note: the usage includes 'е' (U+0435)
  = note: please recheck to make sure their usages are indeed what you want
```

The second family of defenses covers text that *reorders itself on screen*. Unicode bidirectional control characters
(such as U+202E RIGHT-TO-LEFT OVERRIDE) change the display order of text, so code can look different in an editor from
what the compiler reads. After the "Trojan Source" disclosure (Boucher and Anderson, 2021; CVE-2021-42574), Rust
1.56.1 added deny-by-default lints for these characters in literals and comments [VERSION]. Listing
`ch02-05-rust-bidi-literal.rs` contains a real U+202E inside a string literal. Real output (the invisible character is
shown here as `<U+202E>`; the listing contains the character itself):

```text
error: unicode codepoint changing visible direction of text present in literal
 --> src/main.rs:6:24
  |
6 |     let access_level = "user<U+202E> ";
  |                        ^^^^^-^^
  |                        |    |
  |                        |    '\u{202e}'
  |                        this literal contains an invisible unicode text flow control codepoint
  |
  = note: these kind of unicode codepoints change the way text flows on applications that support them, but can cause confusion because they change the order of characters on the screen
  = note: `#[deny(text_direction_codepoint_in_literal)]` on by default
```

The lesson for any language you build: **the lexer decides what text is allowed to look like**, and it's the only stage
that can decide it cheaply.

---

## Practice

### 11. Interview & architecture questions

*Answers are in Appendix A (Part XVII).*

1. What is maximal munch? Give two tokenizations it rules out, and one language feature that makes it awkward.
2. Why do compilers separate lexing from parsing? What property of the token language makes a DFA sufficient?
3. Why should a lexer produce error tokens instead of returning the first error? What does the parser do with them?
4. A teammate wants tokens to own their text (`Ident(String)`) "to avoid lifetime headaches". Quantify the cost with this
   chapter's numbers and propose two designs that avoid both the cost and the headaches.
5. Why does a lexer store byte offsets rather than line and column? When are line and column computed?
6. How are keywords recognized, and why does that make `iffy` an identifier for free?
7. What are rustc's two lexing layers, and why is the lower one a separate, reusable crate?
8. Explain the Trojan Source attack at the level of "what the reviewer sees versus what the compiler reads", and what
   rustc does about it.
9. Java translates `\uXXXX` escapes before lexing. What surprising programs does that allow, and why doesn't Rust have
   the problem?
10. As an architect, what Unicode policy would you choose for identifiers and literals in (a) a general-purpose
    language, (b) a rule DSL whose names come from a catalog? Justify each.

### 12. Exercises

- **Beginner.** Add `>>` and `<<` operators to listing 17.2-1. Which existing arms must they come before, and why? Add
  a test in the style of `maximal_munch`.
- **Intermediate.** Add block comments `/* ... */` to listing 17.2-1, *nested* like Rust's. Why can't a DFA lex nested
  comments, and what do you need instead? Report an unterminated block comment with the span of its opening `/*`.
- **Advanced.** Make listing 17.2-6's table-driven lexer handle identifiers that are keywords, without adding states
  per keyword. (Hint: accept `Ident`, then look the text up.) Then extend the differential test to compare it with
  listing 17.2-1's lexer on random inputs.
- **Systems.** Measure listing 17.2-2's lexer A with and without the `CLASSES` table from listing 17.2-6 (one run each,
  release, noisy). Then run it on input made only of 1-character identifiers separated by single spaces, and on input
  that is one 2 MB comment. Explain the difference using branch prediction and `memchr`.
- **Architecture.** Your company's configuration language accepts arbitrary Unicode in string values (city names,
  merchant names) but only ASCII in keys. Design the lexer-level checks, the review-time tooling, and the alerting so
  that the §10 incident can't recur, without rejecting legitimate data.

### 13. Debugging exercise

A teammate "simplifies" `Lexer::punct` in listing 17.2-1 by listing the one-byte operators first, "because they're more
common" (an unverified sketch of the change):

```rust,ignore
let (kind, len) = match (c, self.b.get(self.pos + 1).copied()) {
    (b'(', _) => (LParen, 1),
    (b'<', _) => (Lt, 1),
    (b'=', _) => (Assign, 1),
    // ... the other one-byte arms ...
    (b'<', Some(b'=')) => (Le, 2),
    (b'=', Some(b'=')) => (EqEq, 2),
    // ... the other two-byte arms ...
```

1. The file still compiles. What does rustc say about the moved arms, and why is that message easy to miss in a
   large build?
2. What tokens does `if x <= 10 { y == 1 }` produce now? Which later stage reports an error, with what message and at
   which position?
3. Which of the listing's five tests fail? Which test would have caught the bug even without a test for `<=`
   specifically?

### 14. Design exercise

**Design Sieve v1's lexer.** Sieve adds durations (`30d`, `1h`, `15m`), percentages (`2.5%`), money
(`EUR 1000.00`, `JPY 5000`), string lists (`["DE", "AT"]`), and comments. Decide:

- The token kinds, and which values are computed at lex time (with what overflow and precision rules).
- Maximal-munch conflicts you must resolve (`1h` versus `1` followed by `h`; `EUR` as a currency versus a feature
  name).
- Error recovery: where does lexing resume after a bad money literal, and after an unterminated string?
- The Unicode policy for identifiers, literals, and comments, including bidirectional controls in comments.
- How the rule editor re-lexes after each keystroke without re-lexing the whole rule set.

Then write the five tests you would add first.
