// verify: debug error:E0502
#[derive(Debug)]
struct User {
    id: u64,
    name: String,
}

struct Registry {
    users: Vec<User>,
}

impl Registry {
    fn find(&self, id: u64) -> Option<&User> {
        self.users.iter().find(|u| u.id == id)
    }

    fn add(&mut self, user: User) {
        self.users.push(user);
    }
}

fn main() {
    let mut registry = Registry {
        users: vec![User { id: 1, name: "ada".into() }],
    };

    let ada = registry.find(1).expect("ada exists");
    registry.add(User { id: 2, name: format!("{}'s colleague", ada.name) });
    println!("found {:?}", ada);
}
