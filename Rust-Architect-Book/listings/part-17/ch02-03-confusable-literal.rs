// verify: debug ok
// Listing 17.2-3: the lexer accepted a string literal that *looks* like "DE" but isn't.
// Two Sieve rules that print identically; one contains U+0415 CYRILLIC CAPITAL LETTER IE.
// The fix is a lexer-level check: flag literals and identifiers that mix scripts or contain
// known look-alikes (a tiny table here; real tools use Unicode's confusables data, UTS #39).

/// A minimal lexer for `name == "literal"` rules: returns (identifier, literal).
fn lex_rule(src: &str) -> (String, String) {
    let (lhs, rhs) = src.split_once("==").expect("rule has ==");
    let lit = rhs.trim().trim_matches('"');
    (lhs.trim().to_string(), lit.to_string())
}

/// A few Latin look-alikes from other scripts (UTS #39 lists thousands).
const CONFUSABLES: &[(char, char, &str)] = &[
    ('\u{0410}', 'A', "CYRILLIC CAPITAL LETTER A"),
    ('\u{0415}', 'E', "CYRILLIC CAPITAL LETTER IE"),
    ('\u{041E}', 'O', "CYRILLIC CAPITAL LETTER O"),
    ('\u{0420}', 'P', "CYRILLIC CAPITAL LETTER ER"),
    ('\u{0421}', 'C', "CYRILLIC CAPITAL LETTER ES"),
    ('\u{0391}', 'A', "GREEK CAPITAL LETTER ALPHA"),
    ('\u{0395}', 'E', "GREEK CAPITAL LETTER EPSILON"),
];

/// The check a rule-language lexer should run on every literal.
fn lint_literal(lit: &str) -> Vec<String> {
    let mut warnings = Vec::new();
    let has_ascii_letters = lit.chars().any(|c| c.is_ascii_alphabetic());
    for (i, c) in lit.char_indices() {
        if let Some((_, looks_like, name)) = CONFUSABLES.iter().find(|(k, _, _)| *k == c) {
            warnings.push(format!(
                "byte {i}: U+{:04X} {name} looks like '{looks_like}'{}",
                c as u32,
                if has_ascii_letters { " (mixed with ASCII letters)" } else { "" }
            ));
        }
    }
    warnings
}

fn main() {
    let rules = ["country == \"DE\"", "country == \"D\u{0415}\""];
    let record_country = "DE"; // what the payment record actually contains

    for rule in rules {
        let (field, lit) = lex_rule(rule);
        let bytes: Vec<String> = lit.bytes().map(|b| format!("{b:02X}")).collect();
        println!("rule   {rule}");
        println!("  field {field}, literal {lit:?}: {} chars, bytes [{}]", lit.chars().count(), bytes.join(" "));
        println!("  matches country=\"{record_country}\"? {}", lit == record_country);
        match lint_literal(&lit).as_slice() {
            [] => println!("  lint: clean"),
            ws => ws.iter().for_each(|w| println!("  lint: warning: {w}")),
        }
    }
}
