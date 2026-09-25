// verify: debug ok
// verify: debug test
// Part X review capstone: one defensible fix of the settlement-report PR (answers in Appendix A, Part X).
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::sync::mpsc;

#[derive(Debug, Clone, PartialEq)]
enum Status {
    Settled,
    Refunded,
    Failed,
}

#[derive(Debug, Clone)]
struct Txn {
    id: u64,
    merchant: String,
    cents: u64,
    currency: &'static str,
    status: Status,
}

#[derive(Debug, PartialEq)]
struct Payout {
    merchant: String,
    cents: u64,
}

#[derive(Debug, PartialEq)]
enum FeeError {
    Malformed(String),
    Duplicate(String),
}

/// Like Collectors.toMap without a merge function: duplicates are an error, and so are malformed rows.
fn load_fees(rows: &[&str]) -> Result<HashMap<String, u32>, FeeError> {
    rows.iter().try_fold(HashMap::new(), |mut m, row| {
        let (k, v) = row.split_once(',').ok_or_else(|| FeeError::Malformed(row.to_string()))?;
        let bps: u32 = v.trim().parse().map_err(|_| FeeError::Malformed(row.to_string()))?;
        match m.entry(k.trim().to_string()) {
            Entry::Vacant(e) => {
                e.insert(bps);
                Ok(m)
            }
            Entry::Occupied(e) => Err(FeeError::Duplicate(e.key().clone())),
        }
    })
}

/// Integer money: basis points applied in integer arithmetic, rounded half up.
fn fee_cents(cents: u64, bps: u32) -> u64 {
    (cents * bps as u64 + 5_000) / 10_000
}

#[derive(Default)]
struct Acc {
    gross: u64,
    fees: u64,
}

#[derive(Debug)]
struct Report {
    settled: usize,
    audited: usize,
    suspicious: Vec<u64>,
    skipped_non_eur: u32,
    fees_cents: u64,
    payouts: Vec<Payout>, // sorted by merchant: deterministic output
}

/// One pass, several outputs: a plain loop over a filtered iterator is clearer than forcing a chain.
fn build_report(txns: &[Txn], fees: &HashMap<String, u32>) -> Result<Report, String> {
    let (mut settled, mut audited, mut skipped_non_eur) = (0, 0, 0);
    let mut suspicious = Vec::new();
    let mut by_merchant: BTreeMap<&str, Acc> = BTreeMap::new();
    for t in txns.iter().filter(|t| t.status == Status::Settled) {
        settled += 1;
        audited += 1; // every settled transaction is screened: no short-circuit decides this
        if t.cents > 1_000_000 {
            suspicious.push(t.id);
        }
        if t.currency != "EUR" {
            skipped_non_eur += 1;
            continue;
        }
        let bps = *fees.get(&t.merchant).ok_or_else(|| format!("no fee row for {}", t.merchant))?;
        let acc = by_merchant.entry(t.merchant.as_str()).or_default(); // borrows the key: no clone per txn
        acc.gross += t.cents;
        acc.fees += fee_cents(t.cents, bps);
    }
    let fees_cents = by_merchant.values().map(|a| a.fees).sum();
    let payouts = by_merchant
        .into_iter()
        .map(|(m, a)| Payout { merchant: m.to_string(), cents: a.gross - a.fees })
        .collect();
    Ok(Report { settled, audited, suspicious, skipped_non_eur, fees_cents, payouts })
}

/// `take` checks its count before pulling, so no payout is ever dropped, whatever the source.
fn send_in_batches(payouts: impl Iterator<Item = Payout>, size: usize) -> Vec<Vec<Payout>> {
    let mut it = payouts;
    let mut sent = Vec::new();
    loop {
        let batch: Vec<Payout> = it.by_ref().take(size).collect();
        if batch.is_empty() {
            return sent;
        }
        sent.push(batch);
    }
}

fn sample_txns() -> Vec<Txn> {
    (0..100_000u64)
        .map(|i| Txn {
            id: i,
            merchant: format!("m-{}", 100 * (1 + i % 12)),
            cents: if i == 5 { 2_500_000 } else { 1 + (i * 37) % 50_000 },
            currency: if i % 7 == 0 { "USD" } else { "EUR" },
            status: match i % 5 {
                0 => Status::Settled,
                1 => Status::Refunded,
                _ => if i % 2 == 0 { Status::Settled } else { Status::Failed },
            },
        })
        .collect()
}

const ROWS: [&str; 12] = [
    "m-100, 290", "m-200, 250", "m-300, 310", "m-400, 275", "m-500, 290", "m-600, 250",
    "m-700, 300", "m-800, 290", "m-900, 250", "m-1000, 310", "m-1100, 290", "m-1200, 199",
];

fn main() {
    let mut with_dup = ROWS.to_vec();
    with_dup.push("m-100, 190");
    println!("partner export with duplicate: {:?}", load_fees(&with_dup).map(|m| m.len()));

    let fees = load_fees(&ROWS).expect("clean schedule");
    let txns = sample_txns();
    let report = build_report(&txns, &fees).expect("every merchant has a fee row");

    // The PR's arithmetic, on the same (clean) schedule, for comparison:
    let truncated: u64 = txns
        .iter()
        .filter(|t| t.status == Status::Settled && t.currency == "EUR")
        .map(|t| (t.cents as f64 * fees[&t.merchant] as f64 / 10_000.0) as u64)
        .sum();

    let (tx, rx) = mpsc::channel();
    for p in report.payouts {
        tx.send(p).unwrap();
    }
    drop(tx);
    let batches = send_in_batches(rx.iter(), 5);

    println!("settled {} / audited {} / suspicious {:?}", report.settled, report.audited, report.suspicious);
    println!("skipped non-EUR: {}", report.skipped_non_eur);
    println!("fees: integer half-up {} cents vs f64 truncation {} cents", report.fees_cents, truncated);
    println!("payout batches: {:?}", batches.iter().map(Vec::len).collect::<Vec<_>>());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn txn(id: u64, merchant: &str, cents: u64, currency: &'static str, status: Status) -> Txn {
        Txn { id, merchant: merchant.to_string(), cents, currency, status }
    }

    #[test]
    fn duplicate_and_malformed_fee_rows_are_rejected() {
        assert_eq!(load_fees(&["a,100", "a,200"]), Err(FeeError::Duplicate("a".into())));
        assert_eq!(load_fees(&["a;100"]), Err(FeeError::Malformed("a;100".into())));
        assert_eq!(load_fees(&["a, x"]), Err(FeeError::Malformed("a, x".into())));
    }

    #[test]
    fn counts_are_complete_and_suspicious_do_not_stop_the_screen() {
        let fees = load_fees(&["a,100"]).unwrap();
        let txns = vec![
            txn(1, "a", 2_000_000, "EUR", Status::Settled), // suspicious, first
            txn(2, "a", 100, "USD", Status::Settled),
            txn(3, "a", 100, "EUR", Status::Failed),
            txn(4, "a", 300, "EUR", Status::Settled),
        ];
        let r = build_report(&txns, &fees).unwrap();
        assert_eq!((r.settled, r.audited, r.skipped_non_eur), (3, 3, 1));
        assert_eq!(r.suspicious, vec![1]);
    }

    #[test]
    fn fee_rounding_is_integer_half_up() {
        assert_eq!(fee_cents(1_005, 290), 29); // 29.145
        assert_eq!(fee_cents(50, 100), 1); // 0.5 rounds up
        assert_eq!(fee_cents(49, 100), 0); // 0.49 rounds down
    }

    #[test]
    fn batching_a_channel_keeps_every_payout() {
        let (tx, rx) = mpsc::channel();
        for i in 0..12 {
            tx.send(Payout { merchant: format!("m{i}"), cents: i }).unwrap();
        }
        drop(tx);
        let sizes: Vec<usize> = send_in_batches(rx.iter(), 5).iter().map(Vec::len).collect();
        assert_eq!(sizes, vec![5, 5, 2]);
    }

    #[test]
    fn missing_fee_row_is_an_error_not_a_panic() {
        let fees = load_fees(&["a,100"]).unwrap();
        let txns = vec![txn(1, "b", 100, "EUR", Status::Settled)];
        assert!(build_report(&txns, &fees).is_err());
    }
}
