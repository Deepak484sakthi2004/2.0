// verify: debug ok
use std::error::Error;
use std::io;
use std::num::ParseIntError;

/// A library error: every variant is something a caller might handle differently.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("account {0} not found")]
    AccountNotFound(u64),
    #[error("insufficient funds: balance {balance}, requested {amount}")]
    InsufficientFunds { balance: i64, amount: i64 },
    #[error("ledger storage unavailable")]
    Storage(#[from] io::Error),
    #[error("corrupt ledger record at line {line}")]
    Corrupt {
        line: usize,
        #[source]
        source: ParseIntError,
    },
}

fn parse_balances(text: &str) -> Result<Vec<i64>, LedgerError> {
    text.lines()
        .enumerate()
        .map(|(i, l)| l.trim().parse::<i64>().map_err(|source| LedgerError::Corrupt { line: i + 1, source }))
        .collect() // Iterator<Item = Result<T, E>> collects into Result<Vec<T>, E>: stops at the first Err
}

fn load(path: &str) -> Result<Vec<i64>, LedgerError> {
    let text = std::fs::read_to_string(path)?; // io::Error -> LedgerError::Storage, via #[from]
    parse_balances(&text)
}

fn debit(balances: &mut [i64], account: u64, amount: i64) -> Result<i64, LedgerError> {
    let slot = balances.get_mut(account as usize).ok_or(LedgerError::AccountNotFound(account))?;
    if *slot < amount {
        return Err(LedgerError::InsufficientFunds { balance: *slot, amount });
    }
    *slot -= amount;
    Ok(*slot)
}

/// Print an error and its chain of causes, each exactly once.
fn report(e: &(dyn Error + 'static)) {
    println!("error: {e}");
    let mut cause = e.source();
    while let Some(c) = cause {
        println!("  caused by: {c}");
        cause = c.source();
    }
}

fn main() {
    let mut balances = parse_balances("500\n120\n").unwrap();
    println!("{:?}", debit(&mut balances, 0, 200));
    for result in [
        debit(&mut balances, 1, 200).map(drop),
        debit(&mut balances, 9, 1).map(drop),
        parse_balances("500\n12O\n").map(drop),
        load("/definitely/not/here/ledger.dat").map(drop),
    ] {
        let e = result.unwrap_err();
        report(&e);
        // The caller can still branch on the variant, which it could not do with a String:
        let retry = matches!(e, LedgerError::Storage(ref io) if io.kind() != io::ErrorKind::NotFound);
        println!("  variant decides: retryable={retry}");
    }
}
