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
    upper: Vec<Vec<Id>>,
    lower: Vec<Vec<Id>>,
    order_edges: usize,
    closure: Option<OrderClosure>,
}

#[derive(Default)]
struct OrderClosure {
    upper: Vec<Vec<u64>>,
    lower: Vec<Vec<u64>>,
    dense_of_raw: Vec<usize>,
    roots: Vec<Id>,
    dirty: bool,
}

fn set_bits(bits: &[u64]) -> Vec<Id> {
    let mut result = Vec::new();
    for (word_index, &word) in bits.iter().enumerate() {
        let mut remaining = word;
        while remaining != 0 {
            let bit = remaining.trailing_zeros() as usize;
            result.push(Id::new(word_index * 64 + bit));
            remaining &= remaining - 1;
        }
    }
    result
}

fn or_row(rows: &mut [Vec<u64>], destination: usize, source: usize) {
    let (destination_row, source_row) = if destination < source {
        let (left, right) = rows.split_at_mut(source);
        (&mut left[destination], &right[0])
    } else {
        let (left, right) = rows.split_at_mut(destination);
        (&mut right[0], &left[source])
    };
    for (word, extra) in destination_row.iter_mut().zip(source_row) {
        *word |= extra;
    }
}

impl UnionFind {
    pub fn with_materialized_closure() -> Self {
        Self {
            closure: Some(OrderClosure {
                dirty: true,
                ..OrderClosure::default()
            }),
            ..Self::default()
        }
    }

    pub fn mkset(&mut self) -> Id {
        let id = Id::new(self.parents.len());
        self.parents.push(id);
        self.upper.push(Vec::new());
        self.lower.push(Vec::new());
        self.classes += 1;
        if let Some(closure) = &mut self.closure {
            closure.dirty = true;
        }
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
        if let Some(closure) = &mut self.closure {
            closure.dirty = true;
        }
        self.parents[a.usize()] = b;
        let upper = std::mem::take(&mut self.upper[a.usize()]);
        let lower = std::mem::take(&mut self.lower[a.usize()]);
        self.upper[b.usize()].extend(upper);
        self.lower[b.usize()].extend(lower);
        self.classes -= 1;
        true
    }
    pub fn assert_le(&mut self, a: Id, b: Id) -> bool {
        let a = self.find_mut(a);
        let b = self.find_mut(b);
        if a == b {
            return false;
        }
        if self.is_le(a, b) {
            return false;
        }
        self.upper[a.usize()].push(b);
        self.lower[b.usize()].push(a);
        self.order_edges += 1;
        if let Some(closure) = &mut self.closure {
            closure.dirty = true;
        }
        true
    }
    /// Record a generated edge without a transitive search. Used by variance
    /// propagation, where each edge has a local witness and duplicates are common.
    pub(crate) fn assert_le_edge(&mut self, a: Id, b: Id) -> bool {
        let a = self.find_mut(a);
        let b = self.find_mut(b);
        if a == b
            || self.upper[a.usize()]
                .iter()
                .any(|&other| self.find(other) == b)
        {
            return false;
        }
        self.upper[a.usize()].push(b);
        self.lower[b.usize()].push(a);
        self.order_edges += 1;
        if let Some(closure) = &mut self.closure {
            closure.dirty = true;
        }
        true
    }
    pub fn is_le(&self, a: Id, b: Id) -> bool {
        let target = self.find(b);
        let source = self.find(a);
        if source == target {
            return true;
        }
        if let Some(closure) = self.closure.as_ref().filter(|closure| !closure.dirty) {
            let source = closure.dense_of_raw[source.usize()];
            let target = closure.dense_of_raw[target.usize()];
            return closure.upper[source][target / 64] & (1 << (target % 64)) != 0;
        }
        self.reaches(source, target)
    }
    pub fn upper_set(&self, id: Id) -> Vec<Id> {
        if let Some(closure) = self.closure.as_ref().filter(|closure| !closure.dirty) {
            let row = closure.dense_of_raw[self.find(id).usize()];
            return set_bits(&closure.upper[row])
                .into_iter()
                .map(|index| closure.roots[index.usize()])
                .collect();
        }
        self.reachable(id, &self.upper)
    }
    pub fn lower_set(&self, id: Id) -> Vec<Id> {
        if let Some(closure) = self.closure.as_ref().filter(|closure| !closure.dirty) {
            let row = closure.dense_of_raw[self.find(id).usize()];
            return set_bits(&closure.lower[row])
                .into_iter()
                .map(|index| closure.roots[index.usize()])
                .collect();
        }
        self.reachable(id, &self.lower)
    }
    pub(crate) fn direct_upper(&self, id: Id) -> Vec<Id> {
        self.direct_neighbors(id, &self.upper)
    }
    pub(crate) fn direct_lower(&self, id: Id) -> Vec<Id> {
        self.direct_neighbors(id, &self.lower)
    }
    fn direct_neighbors(&self, id: Id, edges: &[Vec<Id>]) -> Vec<Id> {
        let root = self.find(id);
        let mut result = Vec::new();
        for &neighbor in &edges[root.usize()] {
            let neighbor = self.find(neighbor);
            if neighbor != root && !result.contains(&neighbor) {
                result.push(neighbor);
            }
        }
        result
    }
    fn reachable(&self, id: Id, edges: &[Vec<Id>]) -> Vec<Id> {
        let mut seen = vec![false; self.parents.len()];
        let mut todo = vec![self.find(id)];
        let mut result = Vec::new();
        while let Some(id) = todo.pop() {
            let id = self.find(id);
            if seen[id.usize()] {
                continue;
            }
            seen[id.usize()] = true;
            result.push(id);
            todo.extend(edges[id.usize()].iter().copied());
        }
        result
    }
    fn reaches(&self, source: Id, target: Id) -> bool {
        let mut seen = vec![false; self.parents.len()];
        let mut todo = vec![source];
        while let Some(id) = todo.pop() {
            let id = self.find(id);
            if id == target {
                return true;
            }
            if seen[id.usize()] {
                continue;
            }
            seen[id.usize()] = true;
            todo.extend(self.upper[id.usize()].iter().copied());
        }
        false
    }
    /// Equate the members of one order cycle; the caller repeats until stable.
    pub fn collapse_order_cycle(&mut self) -> bool {
        if self.order_edges == 0 {
            return false;
        }
        // An iterative depth-first search finds a cycle in O(vertices + edges).
        // Each stack entry stores the next outgoing edge to inspect.
        let mut state = vec![0u8; self.parents.len()];
        let mut stack_position = vec![usize::MAX; self.parents.len()];
        let mut stack: Vec<(Id, usize)> = Vec::new();
        for i in 0..self.parents.len() {
            let root = Id::new(i);
            if !self.is_leader(root) || state[i] != 0 {
                continue;
            }
            state[i] = 1;
            stack_position[i] = 0;
            stack.push((root, 0));
            while let Some((id, next_edge)) = stack.last_mut() {
                let id = *id;
                let edges = &self.upper[id.usize()];
                if *next_edge == edges.len() {
                    state[id.usize()] = 2;
                    stack_position[id.usize()] = usize::MAX;
                    stack.pop();
                    continue;
                }
                let neighbor = self.find(edges[*next_edge]);
                *next_edge += 1;
                if neighbor == id {
                    continue;
                }
                match state[neighbor.usize()] {
                    0 => {
                        state[neighbor.usize()] = 1;
                        stack_position[neighbor.usize()] = stack.len();
                        stack.push((neighbor, 0));
                    }
                    1 => {
                        let start = stack_position[neighbor.usize()];
                        let cycle: Vec<_> = stack[start..].iter().map(|(id, _)| *id).collect();
                        for other in cycle.into_iter().skip(1) {
                            self.union(neighbor, other);
                        }
                        return true;
                    }
                    _ => {}
                }
            }
        }
        false
    }
    /// Rebuild the optional transitive closure after equality and order edges
    /// settle. The collapsed order is a DAG, so one topological pass suffices.
    pub(crate) fn rebuild_closure(&mut self) {
        if !self.closure.as_ref().is_some_and(|closure| closure.dirty) {
            return;
        }
        let roots: Vec<_> = (0..self.parents.len())
            .map(Id::new)
            .filter(|&id| self.is_leader(id))
            .collect();
        let n = roots.len();
        let words = n.div_ceil(64);
        let mut dense_of_raw = vec![usize::MAX; self.parents.len()];
        for (index, &root) in roots.iter().enumerate() {
            dense_of_raw[root.usize()] = index;
        }
        let mut successors = vec![Vec::new(); n];
        let mut indegree = vec![0usize; n];
        for (i, &root) in roots.iter().enumerate() {
            let mut seen = rustc_hash::FxHashSet::default();
            for &other in &self.upper[root.usize()] {
                let other = self.find(other);
                let other = dense_of_raw[other.usize()];
                if other != i && seen.insert(other) {
                    successors[i].push(other);
                    indegree[other] += 1;
                }
            }
        }
        let mut queue: Vec<_> = (0..n).filter(|&i| indegree[i] == 0).collect();
        let mut cursor = 0;
        while cursor < queue.len() {
            let id = queue[cursor];
            cursor += 1;
            for &other in &successors[id] {
                indegree[other] -= 1;
                if indegree[other] == 0 {
                    queue.push(other);
                }
            }
        }
        assert_eq!(queue.len(), self.classes, "order cycle survived rebuild");
        let closure = self.closure.as_mut().unwrap();
        closure.upper.clear();
        closure.lower.clear();
        let mut upper = vec![vec![0u64; words]; n];
        let mut lower = vec![vec![0u64; words]; n];
        for &id in queue.iter().rev() {
            upper[id][id / 64] |= 1 << (id % 64);
            for &other in &successors[id] {
                or_row(&mut upper, id, other);
            }
        }
        for &id in &queue {
            lower[id][id / 64] |= 1 << (id % 64);
            for &other in &successors[id] {
                or_row(&mut lower, other, id);
            }
        }
        closure.upper = upper;
        closure.lower = lower;
        closure.dense_of_raw = dense_of_raw;
        closure.roots = roots;
        closure.dirty = false;
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

#[cfg(test)]
mod tests {
    use super::UnionFind;

    #[test]
    fn searches_both_directions_through_merged_classes() {
        let mut order = UnionFind::default();
        let a = order.mkset();
        let b = order.mkset();
        let c = order.mkset();
        let d = order.mkset();
        assert!(order.assert_le(a, b));
        assert!(order.assert_le(b, c));
        assert!(order.is_le(a, c));
        assert!(!order.is_le(c, a));
        order.union(b, d);
        assert!(order.upper_set(a).contains(&order.find(d)));
        assert!(order.lower_set(c).contains(&order.find(d)));
        assert!(order.assert_le(c, a));
        assert!(order.collapse_order_cycle());
        assert!(order.are_eq(a, c));
        assert!(order.are_eq(a, d));
    }

    #[test]
    fn generated_edges_collapse_only_the_cycle() {
        let mut order = UnionFind::default();
        let a = order.mkset();
        let b = order.mkset();
        let c = order.mkset();
        let outside = order.mkset();
        assert!(order.assert_le_edge(outside, a));
        assert!(order.assert_le_edge(a, b));
        assert!(order.assert_le_edge(b, c));
        assert!(order.assert_le_edge(c, a));
        assert!(order.collapse_order_cycle());
        assert!(order.are_eq(a, b));
        assert!(order.are_eq(b, c));
        assert!(!order.are_eq(outside, a));
        assert!(order.is_le(outside, c));
        assert!(!order.collapse_order_cycle());
    }

    #[test]
    fn materialized_and_sparse_order_agree_after_edges_and_unions() {
        let mut sparse = UnionFind::default();
        let mut materialized = UnionFind::with_materialized_closure();
        let ids: Vec<_> = (0..12).map(|_| sparse.mkset()).collect();
        for _ in &ids {
            materialized.mkset();
        }
        let mut seed = 1u32;
        for step in 0..100 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let a = ids[(seed as usize) % ids.len()];
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let b = ids[(seed as usize) % ids.len()];
            if step % 7 == 0 {
                sparse.union(a, b);
                materialized.union(a, b);
            } else if step % 2 == 0 {
                sparse.assert_le(a, b);
                materialized.assert_le(a, b);
            } else {
                sparse.assert_le_edge(a, b);
                materialized.assert_le_edge(a, b);
            }
            while sparse.collapse_order_cycle() {}
            while materialized.collapse_order_cycle() {}
            for &x in &ids {
                for &y in &ids {
                    assert_eq!(sparse.is_le(x, y), materialized.is_le(x, y));
                }
            }
            materialized.rebuild_closure();
            for &x in &ids {
                for &y in &ids {
                    assert_eq!(sparse.are_eq(x, y), materialized.are_eq(x, y));
                    assert_eq!(sparse.is_le(x, y), materialized.is_le(x, y));
                }
            }
        }
    }
}
