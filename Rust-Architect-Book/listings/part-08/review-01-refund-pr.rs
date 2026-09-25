// verify: debug build
// Part VIII review capstone: the refunds PR, as submitted. It compiles. Find the problems.
#![allow(dead_code)]
use std::collections::HashMap;
use std::error::Error;

pub struct RefundRequest {
    pub payment_id: String,
    pub amount: String,
    pub reason: Option<String>,
}

pub struct Ledger {
    balances: HashMap<String, i64>,
}

pub struct Processor;

impl Processor {
    pub fn refund(&self, payment_id: &str, amount: i64) -> Result<String, String> {
        if payment_id.is_empty() { Err("processor: 503 upstream pool exhausted (host 10.2.7.14)".into()) } else { Ok(format!("rf_{amount}")) }
    }
}

impl Ledger {
    pub fn debit(&mut self, payment_id: &str, amount: i64) -> Result<i64, Box<dyn Error>> {
        let balance = self.balances.get_mut(payment_id).ok_or("no such payment")?;
        if *balance < amount {
            panic!("refund exceeds captured amount");
        }
        *balance -= amount;
        Ok(*balance)
    }
}

pub fn handle_refund(req: RefundRequest, ledger: &mut Ledger, processor: &Processor) -> (u16, String) {
    let amount: i64 = req.amount.parse().unwrap();
    let _reason = req.reason.unwrap_or_default();

    let mut refund_id = String::new();
    for _ in 0..5 {
        match processor.refund(&req.payment_id, amount) {
            Ok(id) => {
                refund_id = id;
                break;
            }
            Err(e) if e.contains("503") => continue,
            Err(e) => return (500, e),
        }
    }

    match ledger.debit(&req.payment_id, amount) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("ERROR debit failed: {e}");
            return (500, format!("debit failed: {e:?}"));
        }
    }

    let _ = write_audit(&req.payment_id, amount);
    (200, refund_id)
}

fn write_audit(payment_id: &str, amount: i64) -> std::io::Result<()> {
    std::fs::write(format!("/var/log/audit/{payment_id}"), amount.to_string())
}
