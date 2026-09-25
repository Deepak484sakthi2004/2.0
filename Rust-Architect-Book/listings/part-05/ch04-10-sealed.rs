// verify: debug error:E0277
mod payment {
    use std::marker::PhantomData;

    mod sealed {
        pub trait Sealed {}
    }
    pub trait State: sealed::Sealed {}

    pub enum Pending {}
    impl sealed::Sealed for Pending {}
    impl State for Pending {}

    pub struct Payment<S: State> {
        pub id: u64,
        _state: PhantomData<S>,
    }
}

// Downstream code tries to invent a state that skips authorization.
pub enum AlreadyCaptured {}
impl payment::State for AlreadyCaptured {}

fn main() {}
