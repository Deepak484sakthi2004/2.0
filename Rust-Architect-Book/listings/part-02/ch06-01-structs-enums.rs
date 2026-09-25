// verify: debug ok
#![allow(dead_code)] // some fields are only printed via derived Debug, which doesn't count as a "read"

#[derive(Debug, Clone, Copy, PartialEq)]
enum Currency {
    Eur,
    Usd,
    Inr,
}

#[derive(Debug, Clone, PartialEq)]
struct Money {
    cents: i64,
    currency: Currency,
}

#[derive(Debug)]
struct AccountId(u64); // a tuple struct: a named wrapper around one value

#[derive(Debug)]
struct Account {
    id: AccountId,
    balance: Money,
    frozen: bool,
}

#[derive(Debug, Clone, Copy)]
enum CardNetwork {
    Visa,
    Mastercard,
}

#[derive(Debug)]
enum PaymentMethod {
    Card { last4: [u8; 4], network: CardNetwork },
    BankTransfer { iban: String },
    Wallet(String),
    Cash,
}

impl Money {
    fn new(cents: i64, currency: Currency) -> Self {
        // an associated function: no `self`
        Money { cents, currency }
    }
    fn is_negative(&self) -> bool {
        // `&self`: reads
        self.cents < 0
    }
}

impl Account {
    fn open(id: u64, currency: Currency) -> Self {
        Account { id: AccountId(id), balance: Money::new(0, currency), frozen: false }
    }
    fn deposit(&mut self, cents: i64) {
        // `&mut self`: modifies in place
        self.balance.cents += cents;
    }
    fn close(self) -> Money {
        // `self`: consumes; the account no longer exists after this call
        self.balance
    }
}

impl PaymentMethod {
    fn fee_bps(&self) -> u32 {
        match self {
            PaymentMethod::Card { network: CardNetwork::Visa, .. } => 180,
            PaymentMethod::Card { network: CardNetwork::Mastercard, .. } => 200,
            PaymentMethod::BankTransfer { .. } => 20,
            PaymentMethod::Wallet(_) => 150,
            PaymentMethod::Cash => 0,
        }
    }
}

fn main() {
    let mut acct = Account::open(42, Currency::Eur);
    acct.deposit(12_500);
    Account::deposit(&mut acct, 500); // the same call, spelled out: a method is a function
    println!("{acct:?}");
    println!("negative? {}", acct.balance.is_negative());

    let methods = [
        PaymentMethod::Card { last4: *b"4242", network: CardNetwork::Visa },
        PaymentMethod::BankTransfer { iban: "DE89370400440532013000".to_string() },
        PaymentMethod::Wallet("paypal:ada".to_string()),
        PaymentMethod::Cash,
    ];
    for m in &methods {
        println!("{:>4} bps  {m:?}", m.fee_bps());
    }

    let final_balance = acct.close();
    println!("closed with {final_balance:?}");
}
