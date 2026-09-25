// verify: debug ok
// A tree with parent links, the Rust way: nodes live in one Vec (the arena),
// links are indices. No Rc, no RefCell, no lifetime parameters.
struct Node {
    name: String,
    parent: Option<usize>,
    children: Vec<usize>,
}

#[derive(Default)]
struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    fn add(&mut self, name: &str, parent: Option<usize>) -> usize {
        let id = self.nodes.len();
        self.nodes.push(Node { name: name.to_string(), parent, children: Vec::new() });
        if let Some(p) = parent {
            self.nodes[p].children.push(id);
        }
        id
    }

    fn path(&self, mut id: usize) -> String {
        let mut parts = vec![self.nodes[id].name.as_str()];
        while let Some(p) = self.nodes[id].parent {
            parts.push(self.nodes[p].name.as_str());
            id = p;
        }
        parts.reverse();
        parts.join("/")
    }
}

fn main() {
    let mut tree = Tree::default();
    let root = tree.add("root", None);
    let etc = tree.add("etc", Some(root));
    let nginx = tree.add("nginx", Some(etc));
    println!("{}", tree.path(nginx));
    println!("root has {} child(ren)", tree.nodes[root].children.len());
}
