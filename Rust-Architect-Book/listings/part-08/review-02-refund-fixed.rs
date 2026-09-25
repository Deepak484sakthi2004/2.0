// verify: debug ok
// Part VIII review capstone: one possible redesign of the refunds PR.
use std::collections::HashMap;

#[derive(Debug, thiserror::Error)]
pub enum RefundError {
    #[error("amount must be a positive whole number of cents")]
    InvalidAmount,
    #[error("payment {0} not found")]
    PaymentNotFound(String),
    #[error("refund of {requested} exceeds the refundable {refundable}")]
    ExceedsCaptured { requested: i64, refundable: i64 },
    #[error("idempotency key reused with a different request")]
    IdempotencyConflict,
    #[error("refund processor unavailable")]
    ProcessorUnavailable,
    #[error("refund processor timed out; the refund is being reconciled")]
    ProcessorTimeout,
}

impl RefundError {
    pub fn status(&self) -> u16 {
        match self {
            Self::InvalidAmount => 400,
            Self::PaymentNotFound(_) => 404,
            Self::ExceedsCaptured { .. } | Self::IdempotencyConflict => 409,
            Self::ProcessorUnavailable => 503,
            Self::ProcessorTimeout => 504,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidAmount => "REFUND_INVALID_AMOUNT",
            Self::PaymentNotFound(_) => "REFUND_PAYMENT_NOT_FOUND",
            Self::ExceedsCaptured { .. } => "REFUND_EXCEEDS_CAPTURED",
            Self::IdempotencyConflict => "REFUND_IDEMPOTENCY_CONFLICT",
            Self::ProcessorUnavailable => "REFUND_PROCESSOR_UNAVAILABLE",
            Self::ProcessorTimeout => "REFUND_PROCESSOR_TIMEOUT",
        }
    }

    pub fn retryable(&self) -> bool {
        matches!(self, Self::ProcessorUnavailable | Self::ProcessorTimeout)
    }
}

/// The processor client reports classified errors, never prose.
#[derive(Debug)]
pub enum ProcessorError {
    Unavailable,
    Timeout,
}

/// A fake processor that deduplicates by key, and fails `fail_next` times first.
#[derive(Default)]
pub struct Processor {
    fail_next: Vec<ProcessorError>,
    by_key: HashMap<String, String>,
    pub calls: u32,
}

impl Processor {
    fn refund(&mut self, key: &str, _amount: i64) -> Result<String, ProcessorError> {
        self.calls += 1;
        if let Some(id) = self.by_key.get(key) {
            return Ok(id.clone());
        }
        if !self.fail_next.is_empty() {
            return Err(self.fail_next.remove(0));
        }
        let id = format!("rf_{}", self.by_key.len() + 1);
        self.by_key.insert(key.to_string(), id.clone());
        Ok(id)
    }
}

#[derive(Default)]
pub struct Refunds {
    refundable: HashMap<String, i64>,
    completed: HashMap<String, (String, i64, String)>, // idempotency key -> (payment, amount, refund id)
    pub outbox: Vec<String>,                           // audit events, written in the same transaction
}

pub struct RefundRequest<'a> {
    pub idempotency_key: &'a str,
    pub payment_id: &'a str,
    pub amount: &'a str,
}

fn parse_amount(raw: &str) -> Result<i64, RefundError> {
    raw.parse::<i64>().ok().filter(|&a| a > 0).ok_or(RefundError::InvalidAmount)
}

impl Refunds {
    pub fn handle(&mut self, req: &RefundRequest, processor: &mut Processor) -> Result<String, RefundError> {
        let amount = parse_amount(req.amount)?;
        if let Some((payment, amt, id)) = self.completed.get(req.idempotency_key) {
            return if payment == req.payment_id && *amt == amount { Ok(id.clone()) } else { Err(RefundError::IdempotencyConflict) };
        }
        // 1. Reserve first: the ledger, not the processor, decides whether the refund is allowed.
        let refundable = self
            .refundable
            .get_mut(req.payment_id)
            .ok_or_else(|| RefundError::PaymentNotFound(req.payment_id.to_string()))?;
        if *refundable < amount {
            return Err(RefundError::ExceedsCaptured { requested: amount, refundable: *refundable });
        }
        *refundable -= amount;

        // 2. Call the processor with a key derived from the client's key: retries can't refund twice.
        let key = format!("refund:{}", req.idempotency_key);
        let mut result = processor.refund(&key, amount);
        for _ in 0..2 {
            if matches!(result, Err(ProcessorError::Unavailable)) {
                result = processor.refund(&key, amount); // (backoff and jitter omitted: Chapter 8.4)
            }
        }
        match result {
            Ok(id) => {
                self.outbox.push(format!("refund {id} payment={} amount={amount}", req.payment_id));
                self.completed.insert(req.idempotency_key.to_string(), (req.payment_id.to_string(), amount, id.clone()));
                Ok(id)
            }
            Err(ProcessorError::Unavailable) => {
                *self.refundable.get_mut(req.payment_id).expect("reserved above") += amount; // release
                Err(RefundError::ProcessorUnavailable)
            }
            Err(ProcessorError::Timeout) => {
                // Outcome unknown: keep the reservation and let reconciliation decide.
                self.outbox.push(format!("reconcile key={key} payment={} amount={amount}", req.payment_id));
                Err(RefundError::ProcessorTimeout)
            }
        }
    }
}

/// The boundary: log once, by class; respond with a code, never with internals.
fn respond(result: Result<String, RefundError>, request_id: &str) -> String {
    match result {
        Ok(id) => format!("200 {}", serde_json::json!({ "refund_id": id, "request_id": request_id })),
        Err(e) => {
            let level = if e.retryable() { "WARN " } else { "INFO " };
            println!("  [{level}] request_id={request_id} code={} {e}", e.code());
            let body = serde_json::json!({ "code": e.code(), "message": e.to_string(), "retryable": e.retryable(), "request_id": request_id });
            format!("{} {body}", e.status())
        }
    }
}

fn main() {
    let mut refunds = Refunds::default();
    refunds.refundable.insert("pay_1".into(), 5_000);
    let mut processor = Processor { fail_next: vec![ProcessorError::Unavailable, ProcessorError::Unavailable], ..Default::default() };

    let requests = [
        ("r1", RefundRequest { idempotency_key: "k1", payment_id: "pay_1", amount: "12.50" }),
        ("r2", RefundRequest { idempotency_key: "k2", payment_id: "pay_9", amount: "100" }),
        ("r3", RefundRequest { idempotency_key: "k3", payment_id: "pay_1", amount: "9000" }),
        ("r4", RefundRequest { idempotency_key: "k4", payment_id: "pay_1", amount: "2000" }),
        ("r5", RefundRequest { idempotency_key: "k4", payment_id: "pay_1", amount: "2000" }), // client retry
        ("r6", RefundRequest { idempotency_key: "k4", payment_id: "pay_1", amount: "3000" }), // key reuse
    ];
    for (request_id, req) in &requests {
        let out = respond(refunds.handle(req, &mut processor), request_id);
        println!("{request_id}: {out}");
    }
    println!("processor calls={} refundable left={} outbox={:?}", processor.calls, refunds.refundable["pay_1"], refunds.outbox);
}
