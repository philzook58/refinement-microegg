use std::str::FromStr;

pub use symbol_table::GlobalSymbol as Symbol;
pub type IndexMap<K, V> = indexmap::IndexMap<K, V, rustc_hash::FxBuildHasher>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Id(u32);
impl Id {
    pub fn new(n: usize) -> Self {
        Self(n as u32)
    }
    pub fn usize(self) -> usize {
        self.0 as usize
    }
}
impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Default)]
pub struct UnionFind {
    parents: Vec<Id>,
    classes: usize,
}
impl UnionFind {
    pub fn mkset(&mut self) -> Id {
        let id = Id::new(self.parents.len());
        self.parents.push(id);
        self.classes += 1;
        id
    }
    pub fn find(&self, mut id: Id) -> Id {
        while self.parents[id.usize()] != id {
            id = self.parents[id.usize()];
        }
        id
    }
    pub fn find_mut(&mut self, id: Id) -> Id {
        let root = self.find(id);
        let mut current = id;
        while current != root {
            let next = self.parents[current.usize()];
            self.parents[current.usize()] = root;
            current = next;
        }
        root
    }
    pub fn union(&mut self, a: Id, b: Id) -> bool {
        let a = self.find_mut(a);
        let b = self.find_mut(b);
        if a == b {
            return false;
        }
        self.parents[a.usize()] = b;
        self.classes -= 1;
        true
    }
    pub fn are_eq(&self, a: Id, b: Id) -> bool {
        self.find(a) == self.find(b)
    }
    pub fn is_leader(&self, id: Id) -> bool {
        self.find(id) == id
    }
    pub fn n_classes(&self) -> usize {
        self.classes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sexp {
    Atom(Symbol),
    List(Vec<Sexp>),
}
impl std::fmt::Display for Sexp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Atom(atom) => write!(f, "{atom}"),
            Self::List(items) => write!(f, "({})", DisplayIter(items, " ")),
        }
    }
}
pub fn sexp(s: &str) -> Sexp {
    s.parse().unwrap()
}
impl FromStr for Sexp {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let forms = crate::script::parse(s)?;
        if forms.len() == 1 {
            Ok(forms.into_iter().next().unwrap())
        } else {
            Err("expected exactly one s-expression".into())
        }
    }
}

pub struct DisplayIter<I>(pub I, pub &'static str);
impl<I: Clone + IntoIterator> std::fmt::Display for DisplayIter<I>
where
    I::Item: std::fmt::Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut items = self.0.clone().into_iter();
        if let Some(item) = items.next() {
            write!(f, "{item}")?;
        }
        for item in items {
            write!(f, "{}{item}", self.1)?;
        }
        Ok(())
    }
}

#[derive(Default, Debug, Clone)]
pub struct Subst(Vec<(Symbol, Id)>);
impl Subst {
    pub fn with(&self, name: Symbol, id: Id) -> Option<Self> {
        match self.0.iter().find(|(key, _)| *key == name) {
            Some((_, old)) if *old == id => Some(self.clone()),
            Some(_) => None,
            None => {
                let mut next = self.clone();
                next.0.push((name, id));
                Some(next)
            }
        }
    }
}
impl std::ops::Index<Symbol> for Subst {
    type Output = Id;
    fn index(&self, name: Symbol) -> &Id {
        &self
            .0
            .iter()
            .find(|(key, _)| *key == name)
            .expect("unbound pattern variable")
            .1
    }
}
pub fn var_parse_or<T: FromStr>(key: &str, default: T) -> T
where
    T::Err: std::fmt::Debug,
{
    std::env::var(key).map_or(default, |value| value.parse().unwrap())
}
