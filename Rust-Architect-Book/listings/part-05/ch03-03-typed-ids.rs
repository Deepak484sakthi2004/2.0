// verify: debug ok
use std::collections::HashMap;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::mem::size_of;

/// A typed identifier: the same u64 at run time, a different type per entity at compile time.
/// `PhantomData<fn() -> T>`: covariant in T, always Send + Sync, and does not claim to own a T.
pub struct Id<T> {
    raw: u64,
    _entity: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    pub const fn new(raw: u64) -> Self {
        Id { raw, _entity: PhantomData }
    }
    pub const fn get(self) -> u64 {
        self.raw
    }
}

// Manual impls: `#[derive]` would add `T: Clone`, `T: PartialEq`, ... bounds (Chapter 5.3 §13).
impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Id<T> {}
impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}
impl<T> Eq for Id<T> {}
impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.raw.hash(h)
    }
}
impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = std::any::type_name::<T>().rsplit("::").next().unwrap_or("?");
        write!(f, "{name}#{}", self.raw)
    }
}

// Entities. Only the type names matter to Id<T>.
pub struct Tenant {
    pub name: String,
}
pub struct Order {
    pub tenant: Id<Tenant>,
    pub cents: i64,
}

pub type TenantId = Id<Tenant>;
pub type OrderId = Id<Order>;

fn orders_for(tenant: TenantId, orders: &HashMap<OrderId, Order>) -> Vec<OrderId> {
    let mut ids: Vec<OrderId> = orders.iter().filter(|(_, o)| o.tenant == tenant).map(|(id, _)| *id).collect();
    ids.sort_by_key(|id| id.get());
    ids
}

fn main() {
    let acme = TenantId::new(7);
    let globex = TenantId::new(9);
    let tenants = HashMap::from([(acme, Tenant { name: "acme".into() }), (globex, Tenant { name: "globex".into() })]);
    let orders = HashMap::from([
        (OrderId::new(7), Order { tenant: globex, cents: 1_200 }), // note: order #7 belongs to tenant #9
        (OrderId::new(8), Order { tenant: acme, cents: 4_999 }),
        (OrderId::new(9), Order { tenant: acme, cents: 150 }),
    ]);
    for id in orders_for(acme, &orders) {
        println!("{:?} of {} -> {:?}: {} cents", acme, tenants[&acme].name, id, orders[&id].cents);
    }
    println!("size_of: u64={} Id<Order>={} Option<Id<Order>>={}", size_of::<u64>(), size_of::<OrderId>(), size_of::<Option<OrderId>>());
}
