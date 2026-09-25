// verify: debug ok
mod payment {
    use std::marker::PhantomData;

    mod sealed {
        pub trait Sealed {}
    }

    /// The set of states is closed: `Sealed` is unnameable outside this module.
    pub trait State: sealed::Sealed {
        const NAME: &'static str;
    }

    // Uninhabited marker types: they exist only at compile time. No value of them can ever be made.
    pub enum Pending {}
    pub enum Authorized {}
    pub enum Captured {}
    pub enum Refunded {}
    pub enum Failed {}

    macro_rules! states {
        ($($s:ident),*) => {$(
            impl sealed::Sealed for $s {}
            impl State for $s { const NAME: &'static str = stringify!($s); }
        )*};
    }
    states!(Pending, Authorized, Captured, Refunded, Failed);

    pub struct Payment<S: State> {
        id: u64,
        cents: i64,
        _state: PhantomData<S>,
    }

    impl<S: State> Payment<S> {
        pub fn id(&self) -> u64 {
            self.id
        }
        pub fn state(&self) -> &'static str {
            S::NAME
        }
        // Private: the only way to change state is one of the transition methods below.
        fn into_state<T: State>(self) -> Payment<T> {
            Payment { id: self.id, cents: self.cents, _state: PhantomData }
        }
    }

    impl Payment<Pending> {
        pub fn new(id: u64, cents: i64) -> Self {
            Payment { id, cents, _state: PhantomData }
        }
        pub fn authorize(self) -> Payment<Authorized> {
            self.into_state()
        }
        pub fn fail(self) -> Payment<Failed> {
            self.into_state()
        }
    }

    impl Payment<Authorized> {
        pub fn capture(self) -> Payment<Captured> {
            self.into_state()
        }
        pub fn fail(self) -> Payment<Failed> {
            self.into_state()
        }
    }

    impl Payment<Captured> {
        pub fn refund(self) -> Payment<Refunded> {
            self.into_state()
        }
        pub fn amount(&self) -> i64 {
            self.cents
        }
    }
    // Payment<Refunded> and Payment<Failed> have no transition methods: they are terminal.
}

use payment::{Payment, Pending};

fn main() {
    let p = Payment::<Pending>::new(42, 4_999);
    println!("#{} {}", p.id(), p.state());
    let p = p.authorize();
    println!("#{} --authorize--> {}", p.id(), p.state());
    let p = p.capture();
    println!("#{} --capture--> {} ({} cents)", p.id(), p.state(), p.amount());
    let p = p.refund();
    println!("#{} --refund--> {}", p.id(), p.state());

    let q = Payment::<Pending>::new(43, 150).authorize().fail();
    println!("#{} --authorize--> --fail--> {}", q.id(), q.state());
    let r = Payment::<Pending>::new(44, 10).fail();
    println!("#{} --fail--> {}", r.id(), r.state());

    use std::mem::size_of;
    println!(
        "size_of: Payment<Pending>={} Payment<Captured>={} (u64, i64)={}",
        size_of::<Payment<Pending>>(),
        size_of::<Payment<payment::Captured>>(),
        size_of::<(u64, i64)>()
    );
}
