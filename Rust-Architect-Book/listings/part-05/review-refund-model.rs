// verify: debug ok
// Part V review, model answer: Meridian's refund workflow, redesigned with this Part's tools.
#![allow(dead_code)] // the model defines variants this short demo doesn't exercise
mod ids {
    use std::fmt;
    use std::marker::PhantomData;
    use std::num::NonZeroU64;

    /// Typed ID: NonZeroU64 so Option<Id<T>> is 8 bytes; fn() -> T so it is Send + Sync and covariant.
    pub struct Id<T> {
        raw: NonZeroU64,
        _entity: PhantomData<fn() -> T>,
    }
    impl<T> Id<T> {
        /// pub(crate): IDs come from the persistence layer or a parser, not from arithmetic.
        pub(crate) fn from_raw(raw: u64) -> Option<Self> {
            NonZeroU64::new(raw).map(|raw| Id { raw, _entity: PhantomData })
        }
    }
    impl<T> Clone for Id<T> {
        fn clone(&self) -> Self {
            *self
        }
    }
    impl<T> Copy for Id<T> {}
    impl<T> PartialEq for Id<T> {
        fn eq(&self, o: &Self) -> bool {
            self.raw == o.raw
        }
    }
    impl<T> Eq for Id<T> {}
    impl<T> fmt::Debug for Id<T> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let name = std::any::type_name::<T>().rsplit("::").next().unwrap_or("?");
            write!(f, "{name}#{}", self.raw)
        }
    }

    pub enum RefundEntity {}
    pub enum PaymentEntity {}
    pub enum UserEntity {}
    pub type RefundId = Id<RefundEntity>;
    pub type PaymentId = Id<PaymentEntity>;
    pub type UserId = Id<UserEntity>;
}

mod refund {
    use crate::ids::{PaymentId, RefundId, UserId};
    use std::num::NonZeroU64;

    /// A refund amount in minor units: positive by construction.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct RefundAmount(NonZeroU64);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u8)]
    pub enum Currency {
        Eur = 1,
        Usd = 2,
    }

    /// What the refund service knows about the captured payment it refunds.
    pub struct CapturedPayment {
        pub id: PaymentId,
        pub captured_minor: u64,
        pub currency: Currency,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum RejectReason {
        OutsideWindow,
        SuspectedFraud,
        DuplicateRequest,
    }

    #[derive(Debug, PartialEq)]
    pub enum RefundError {
        NotPositive,
        ExceedsCapture { requested: u64, captured: u64 },
        SelfApproval,
    }

    // States carry exactly the data that exists in them.
    #[derive(Debug)]
    pub struct Requested {
        requested_by: UserId,
    }
    #[derive(Debug)]
    pub struct Approved {
        approved_by: UserId,
    }
    #[derive(Debug)]
    pub struct Rejected {
        reason: RejectReason,
    }
    #[derive(Debug)]
    pub struct Sent {
        psp_ref: String,
    }
    #[derive(Debug)]
    pub struct Settled {
        psp_ref: String,
        settled_at_unix: u64,
    }

    #[derive(Debug)]
    pub struct Refund<S> {
        id: RefundId,
        payment: PaymentId,
        amount: RefundAmount,
        currency: Currency,
        state: S,
    }

    impl<S> Refund<S> {
        pub fn id(&self) -> RefundId {
            self.id
        }
        fn with<T>(self, state: T) -> Refund<T> {
            Refund { id: self.id, payment: self.payment, amount: self.amount, currency: self.currency, state }
        }
    }

    impl Refund<Requested> {
        /// Parse, don't validate: the only way to create a refund checks the amount against the capture.
        pub fn request(id: RefundId, of: &CapturedPayment, minor: u64, by: UserId) -> Result<Self, RefundError> {
            let amount = NonZeroU64::new(minor).ok_or(RefundError::NotPositive)?;
            if minor > of.captured_minor {
                return Err(RefundError::ExceedsCapture { requested: minor, captured: of.captured_minor });
            }
            Ok(Refund { id, payment: of.id, amount: RefundAmount(amount), currency: of.currency, state: Requested { requested_by: by } })
        }

        /// Four-eyes rule: the approver must differ from the requester. On failure the request is handed back.
        pub fn approve(self, approver: UserId) -> Result<Refund<Approved>, (Self, RefundError)> {
            if approver == self.state.requested_by {
                return Err((self, RefundError::SelfApproval));
            }
            Ok(self.with(Approved { approved_by: approver }))
        }

        pub fn reject(self, reason: RejectReason) -> Refund<Rejected> {
            self.with(Rejected { reason })
        }
    }

    impl Refund<Approved> {
        pub fn approved_by(&self) -> UserId {
            self.state.approved_by
        }
        pub fn send(self, psp_ref: &str) -> Refund<Sent> {
            self.with(Sent { psp_ref: psp_ref.to_string() })
        }
    }

    impl Refund<Rejected> {
        pub fn reason(&self) -> RejectReason {
            self.state.reason
        }
    }

    impl Refund<Sent> {
        /// Settlement arrives from a PSP webhook: see AnyRefund for the run-time edge.
        pub fn settle(self, at_unix: u64) -> Refund<Settled> {
            let psp_ref = self.state.psp_ref.clone();
            self.with(Settled { psp_ref, settled_at_unix: at_unix })
        }
    }

    impl Refund<Settled> {
        pub fn summary(&self) -> String {
            format!("{:?} of {:?}: {} {:?} settled at {} (psp {})",
                self.id, self.payment, self.amount.0, self.currency, self.state.settled_at_unix, self.state.psp_ref)
        }
    }

    /// The run-time view for rows and webhooks.
    pub enum AnyRefund {
        Requested(Refund<Requested>),
        Approved(Refund<Approved>),
        Rejected(Refund<Rejected>),
        Sent(Refund<Sent>),
        Settled(Refund<Settled>),
    }

    impl AnyRefund {
        /// A PSP webhook "settled(psp_ref, at)": legal only for a Sent refund with a matching reference.
        pub fn on_settled_webhook(self, psp_ref: &str, at: u64) -> Result<AnyRefund, (AnyRefund, String)> {
            match self {
                AnyRefund::Sent(r) if r.state.psp_ref == psp_ref => Ok(AnyRefund::Settled(r.settle(at))),
                AnyRefund::Sent(r) => {
                    let msg = format!("psp ref mismatch for {:?}", r.id());
                    Err((AnyRefund::Sent(r), msg))
                }
                other => {
                    let msg = format!("settlement webhook for a refund in state {}", other.state_column());
                    Err((other, msg))
                }
            }
        }

        pub fn state_column(&self) -> &'static str {
            match self {
                AnyRefund::Requested(_) => "REQUESTED",
                AnyRefund::Approved(_) => "APPROVED",
                AnyRefund::Rejected(_) => "REJECTED",
                AnyRefund::Sent(_) => "SENT",
                AnyRefund::Settled(_) => "SETTLED",
            }
        }
    }
}

use ids::{Id, PaymentId, RefundId, UserId};
use refund::{AnyRefund, CapturedPayment, Currency, RejectReason, Refund, Requested};

fn main() {
    let pay = CapturedPayment { id: PaymentId::from_raw(42).unwrap(), captured_minor: 4_999, currency: Currency::Eur };
    let (alice, bob): (UserId, UserId) = (Id::from_raw(7).unwrap(), Id::from_raw(9).unwrap());

    // Parsing the request: amount checks happen once, here.
    for minor in [0, 6_000] {
        println!("request {minor}: {:?}", Refund::<Requested>::request(RefundId::from_raw(1).unwrap(), &pay, minor, alice).map(|r| r.id()));
    }
    let r = Refund::request(RefundId::from_raw(2).unwrap(), &pay, 1_200, alice).expect("valid request");

    // Four-eyes: self-approval hands the request back.
    let r = match r.approve(alice) {
        Ok(_) => unreachable!(),
        Err((back, e)) => {
            println!("approve by requester: {e:?}");
            back
        }
    };
    let r = r.approve(bob).expect("second pair of eyes");
    println!("approved by {:?}", r.approved_by());
    let sent = r.send("psp-77");

    // The webhook path is run-time data: it goes through AnyRefund.
    let any = AnyRefund::Sent(sent);
    let any = match any.on_settled_webhook("psp-99", 1_760_000_000) {
        Ok(_) => unreachable!(),
        Err((back, e)) => {
            println!("webhook rejected: {e}; still {}", back.state_column());
            back
        }
    };
    match any.on_settled_webhook("psp-77", 1_760_000_000) {
        Ok(AnyRefund::Settled(s)) => println!("{}", s.summary()),
        Ok(other) => println!("unexpected state {}", other.state_column()),
        Err((_, e)) => println!("webhook rejected: {e}"),
    }

    let rejected = Refund::request(RefundId::from_raw(3).unwrap(), &pay, 100, alice).unwrap().reject(RejectReason::DuplicateRequest);
    println!("{:?} rejected: {:?}", rejected.id(), rejected.reason());
    match AnyRefund::Rejected(rejected).on_settled_webhook("psp-1", 0) {
        Ok(_) => unreachable!(),
        Err((_, e)) => println!("webhook rejected: {e}"),
    }

    use std::mem::size_of;
    println!(
        "size_of: RefundId={} Option<RefundId>={} Refund<Requested>={} Refund<Settled>={} AnyRefund={}",
        size_of::<RefundId>(),
        size_of::<Option<RefundId>>(),
        size_of::<Refund<Requested>>(),
        size_of::<Refund<refund::Settled>>(),
        size_of::<AnyRefund>()
    );
}
