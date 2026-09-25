// verify: debug ok
// Part X review capstone: the PR as submitted. It compiles, runs, and prints plausible numbers.
use std::collections::HashMap;
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

#[derive(Debug)]
struct Payout {
    merchant: String,
    cents: u64,
}

struct Report {
    settled: Vec<Txn>, // kept for the ops dashboard until tomorrow's run
    payouts: Vec<Payout>,
    fees_cents: u64,
    skipped_non_eur: u32,
    audited: Vec<u64>, // compliance: every settled transaction must be screened
}

/// Parses "merchant,bps" rows from the partner's fee-schedule export.
fn load_fees(rows: &[&str]) -> HashMap<String, u32> {
    rows.iter()
        .map(|r| {
            let (m, bps) = r.split_once(',').unwrap();
            (m.to_string(), bps.trim().parse().unwrap())
        })
        .collect()
}

fn build_report(txns: Vec<Txn>, fees: &HashMap<String, u32>) -> Report {
    let mut skipped_non_eur = 0;
    let settled: Vec<Txn> = txns.into_iter().filter(|t| t.status == Status::Settled).collect();
    let eur: Vec<&Txn> = settled
        .iter()
        .filter(move |t| {
            if t.currency != "EUR" {
                skipped_non_eur += 1;
                return false;
            }
            true
        })
        .collect();

    let mut by_merchant: HashMap<String, Vec<&Txn>> = HashMap::new();
    for t in eur.iter() {
        by_merchant.entry(t.merchant.clone()).or_default().push(t);
    }
    let mut fees_cents = 0;
    let payouts: Vec<Payout> = by_merchant
        .into_iter()
        .map(|(m, ts)| {
            let bps = fees[&m] as f64;
            let gross: u64 = ts.iter().map(|t| t.cents).sum();
            let fee: u64 = ts.iter().map(|t| (t.cents as f64 * bps / 10_000.0) as u64).sum();
            fees_cents += fee;
            Payout { merchant: m, cents: gross - fee }
        })
        .collect();

    let mut audited = Vec::new();
    let _suspicious = settled.iter().map(|t| {
        audited.push(t.id);
        t
    })
    .any(|t| t.cents > 1_000_000);

    Report { settled, payouts, fees_cents, skipped_non_eur, audited }
}

/// Sends payouts to the bank API in batches of `size` (the bank's per-request limit).
fn send_in_batches(payouts: &mut impl Iterator<Item = Payout>, size: usize) -> Vec<Vec<Payout>> {
    let mut sent = Vec::new();
    loop {
        let batch: Vec<Payout> = payouts.by_ref().zip(0..size).map(|(p, _)| p).collect();
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

fn main() {
    let fee_rows = [
        "m-100, 290", "m-200, 250", "m-300, 310", "m-400, 275", "m-500, 290", "m-600, 250",
        "m-700, 300", "m-800, 290", "m-900, 250", "m-1000, 310", "m-1100, 290", "m-1200, 199",
        "m-100, 190", // the partner export repeated m-100 with a promotional rate
    ];
    let fees = load_fees(&fee_rows);
    let report = build_report(sample_txns(), &fees);

    let (tx, rx) = mpsc::channel(); // in production, payouts stream to the sender task over a channel
    for p in report.payouts {
        tx.send(p).unwrap();
    }
    drop(tx);
    let batches = send_in_batches(&mut rx.iter(), 5);

    let settled_eur = report.settled.iter().filter(|t| t.currency == "EUR").count();
    println!("fee for m-100:          {} bps", fees["m-100"]);
    println!("settled kept:           len {} / capacity {} ({} MB held)",
        report.settled.len(), report.settled.capacity(),
        report.settled.capacity() * std::mem::size_of::<Txn>() / 1_000_000);
    println!("skipped non-EUR:        {} (actual {})", report.skipped_non_eur, report.settled.len() - settled_eur);
    println!("audited:                {} of {} settled", report.audited.len(), report.settled.len());
    println!("fees charged:           {} cents", report.fees_cents);
    println!("payout batches sent:    {} batches, {} payouts (merchants with EUR volume: 12)",
        batches.len(), batches.iter().map(Vec::len).sum::<usize>());
}
