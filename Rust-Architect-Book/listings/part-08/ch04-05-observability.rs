// verify: debug ok
use std::collections::BTreeMap;
use std::error::Error;
use std::io;

#[derive(Debug, thiserror::Error)]
pub enum PaymentError {
    #[error("card declined: {reason}")]
    Declined { reason: &'static str },
    #[error("card processor unavailable")]
    ProcessorUnavailable,
    #[error("ledger write failed")]
    Ledger(#[source] io::Error),
}

impl PaymentError {
    fn code(&self) -> &'static str {
        match self {
            Self::Declined { .. } => "PAY_DECLINED",
            Self::ProcessorUnavailable => "PAY_PROCESSOR_UNAVAILABLE",
            Self::Ledger(_) => "PAY_INTERNAL",
        }
    }
    fn class(&self) -> &'static str {
        match self {
            Self::Declined { .. } => "rejected",
            Self::ProcessorUnavailable => "transient",
            Self::Ledger(_) => "internal",
        }
    }
}

/// The full cause chain, joined: for logs only, never for clients.
fn chain(e: &dyn Error) -> String {
    let mut s = e.to_string();
    let mut cur = e.source();
    while let Some(c) = cur {
        s.push_str(": ");
        s.push_str(&c.to_string());
        cur = c.source();
    }
    s
}

/// The one place a request's error is logged and counted: the service boundary.
fn record(e: &PaymentError, request_id: &str, metrics: &mut BTreeMap<(&'static str, &'static str), u64>) {
    *metrics.entry((e.code(), e.class())).or_default() += 1;
    match e.class() {
        // A declined card is the system working correctly: not an ERROR, never pages anyone.
        "rejected" => tracing::info!(request_id, code = e.code(), class = e.class(), "payment rejected"),
        "transient" => tracing::warn!(request_id, code = e.code(), class = e.class(), error = %e, "payment failed, retryable"),
        _ => tracing::error!(request_id, code = e.code(), class = e.class(), error = %chain(e), "payment failed"),
    }
}

fn main() {
    tracing_subscriber::fmt().without_time().with_target(false).with_ansi(false).with_writer(io::stdout).init();

    let mut metrics = BTreeMap::new();
    let failures = [
        ("req-01", PaymentError::Declined { reason: "insufficient_funds" }),
        ("req-02", PaymentError::ProcessorUnavailable),
        ("req-03", PaymentError::Declined { reason: "expired_card" }),
        ("req-04", PaymentError::Ledger(io::Error::other("fsync failed: EIO"))),
    ];
    for (id, e) in &failures {
        record(e, id, &mut metrics);
    }
    // Prometheus exposition format. Labels are low-cardinality codes: never messages, amounts or user ids.
    for ((code, class), n) in &metrics {
        println!("payment_errors_total{{code=\"{code}\",class=\"{class}\"}} {n}");
    }
}
