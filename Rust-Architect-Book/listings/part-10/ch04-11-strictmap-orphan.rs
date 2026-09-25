// verify: debug error:E0117
// Answer-key check (Chapter 10.4, advanced exercise): can collect() itself fail by targeting Result<StrictMap, _>?
use std::collections::HashMap;

struct StrictMap<K, V>(HashMap<K, V>);
struct DupError;

impl<K: std::hash::Hash + Eq, V> FromIterator<(K, V)> for Result<StrictMap<K, V>, DupError> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut m = HashMap::new();
        for (k, v) in iter {
            if m.insert(k, v).is_some() {
                return Err(DupError);
            }
        }
        Ok(StrictMap(m))
    }
}

fn main() {}
