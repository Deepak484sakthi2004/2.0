// verify: debug ok
use std::error::Error;
use std::io;

/// Meridian payments: the domain error. One enum, one place where every failure is classified.
#[derive(Debug, thiserror::Error)]
pub enum PaymentError {
    #[error("amount must be positive, got {0}")]
    InvalidAmount(i64),
    #[error("card declined: {reason}")]
    Declined { reason: &'static str },
    #[error("idempotency key reused with a different request")]
    IdempotencyConflict,
    #[error("rate limited, retry after {retry_after_ms} ms")]
    RateLimited { retry_after_ms: u64 },
    #[error("card processor unavailable")]
    ProcessorUnavailable,
    #[error("card processor timed out")]
    ProcessorTimeout,
    #[error("ledger write failed")]
    Ledger(#[source] io::Error),
}

/// What the failure means for the caller, which is what the caller actually needs to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// The request is wrong or refused. The same request will fail the same way: don't retry.
    Rejected,
    /// It did not happen, and may succeed later: retry with backoff.
    Transient,
    /// We don't know whether it happened: retry ONLY with the same idempotency key.
    Ambiguous,
    /// Our bug or a broken dependency invariant: don't retry blindly, alert a human.
    Internal,
}

impl PaymentError {
    pub fn class(&self) -> Class {
        match self {
            Self::InvalidAmount(_) | Self::Declined { .. } | Self::IdempotencyConflict => Class::Rejected,
            Self::RateLimited { .. } | Self::ProcessorUnavailable => Class::Transient,
            Self::ProcessorTimeout => Class::Ambiguous,
            Self::Ledger(_) => Class::Internal,
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            Self::InvalidAmount(_) => 400,
            Self::Declined { .. } => 402,
            Self::IdempotencyConflict => 409,
            Self::RateLimited { .. } => 429,
            Self::ProcessorUnavailable => 503,
            Self::ProcessorTimeout => 504,
            Self::Ledger(_) => 500,
        }
    }

    /// Stable, machine-readable, documented. Messages may change; codes may not.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidAmount(_) => "PAY_INVALID_AMOUNT",
            Self::Declined { .. } => "PAY_DECLINED",
            Self::IdempotencyConflict => "PAY_IDEMPOTENCY_CONFLICT",
            Self::RateLimited { .. } => "PAY_RATE_LIMITED",
            Self::ProcessorUnavailable => "PAY_PROCESSOR_UNAVAILABLE",
            Self::ProcessorTimeout => "PAY_PROCESSOR_TIMEOUT",
            Self::Ledger(_) => "PAY_INTERNAL",
        }
    }

    /// What leaves the service. Internal details (file paths, SQL, stack traces) never do.
    pub fn client_body(&self, request_id: &str) -> serde_json::Value {
        let message = match self.class() {
            Class::Internal => "internal error".to_string(),
            _ => self.to_string(),
        };
        let mut body = serde_json::json!({
            "code": self.code(),
            "message": message,
            "retryable": matches!(self.class(), Class::Transient | Class::Ambiguous),
            "request_id": request_id,
        });
        if let Self::RateLimited { retry_after_ms } = self {
            body["retry_after_ms"] = (*retry_after_ms).into();
        }
        body
    }
}

fn main() {
    let errors = [
        PaymentError::InvalidAmount(-500),
        PaymentError::Declined { reason: "insufficient_funds" },
        PaymentError::IdempotencyConflict,
        PaymentError::RateLimited { retry_after_ms: 250 },
        PaymentError::ProcessorUnavailable,
        PaymentError::ProcessorTimeout,
        PaymentError::Ledger(io::Error::other("disk quota exceeded on /var/lib/ledger/wal-000017")),
    ];
    for e in &errors {
        println!("{:<26} {:<10} {}", e.code(), format!("{:?}", e.class()), e.http_status());
    }
    let internal = errors.last().unwrap();
    println!("log:    {internal} | cause: {}", internal.source().unwrap());
    println!("client: {}", internal.client_body("req-7f3a"));
    println!("client: {}", errors[3].client_body("req-7f3b"));
}
