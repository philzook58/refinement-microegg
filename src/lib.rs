/*!

A simple e-graph in the style of egg's SymbolLang.

 */
pub mod script;
pub mod util;
use crate::util::*;
use rustc_hash::FxHashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};
#[cfg(target_arch = "wasm32")]
use web_time::{Duration, Instant};

/// Timing for one rewrite round after the graph has been rebuilt.
pub struct StepProfile {
    pub matching: Duration,
    pub applying: Duration,
    pub rebuilding: Duration,
    pub matches: usize,
    pub new_nodes: usize,
    pub unions: usize,
    pub changed: bool,
}

// The basics
#[derive(PartialEq, Eq, Hash, Clone, Debug)]
pub struct Node {
    f: Symbol,
    args: Vec<Id>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variance {
    Covariant,
    Contravariant,
    Invariant,
}

/// Which classes may be used relative to a starting class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Below,
    Exact,
    Above,
}

impl Variance {
    fn act(self, mode: Mode) -> Mode {
        match self {
            Self::Covariant => mode,
            Self::Contravariant => match mode {
                Mode::Below => Mode::Above,
                Mode::Exact => Mode::Exact,
                Mode::Above => Mode::Below,
            },
            Self::Invariant => Mode::Exact,
        }
    }
}

impl Mode {
    fn holds(self, uf: &UnionFind, source: Id, candidate: Id) -> bool {
        match self {
            Self::Below => uf.is_le(candidate, source),
            Self::Exact => uf.are_eq(source, candidate),
            Self::Above => uf.is_le(source, candidate),
        }
    }
}

/// Experimental ways to propagate declared variance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VarianceStrategy {
    Eager,
    #[default]
    Cartesian,
    Pairwise,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct VarianceProfile {
    pub time: Duration,
    pub candidates: u64,
    pub edges_added: u64,
}

impl std::fmt::Display for Node {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.args.is_empty() {
            write!(f, "{}", self.f)
        } else {
            write!(f, "({} {})", self.f, DisplayIter(&self.args, " "))
        }
    }
}

#[derive(Default)]
pub struct EGraph {
    // maps e-nodes to the class they are in
    nodes: IndexMap<Node, Id>,
    // the reverse map, from class to the e-nodes in that class
    rev: IndexMap<Id, Vec<Node>>,
    uf: UnionFind,
    variance: IndexMap<Symbol, Vec<Variance>>,
    variance_strategy: VarianceStrategy,
    variance_profile: VarianceProfile,
    needs_rebuild: bool,
}

impl EGraph {
    pub fn set_variance_strategy(&mut self, strategy: VarianceStrategy) {
        self.variance_strategy = strategy;
    }

    /// Perform at most `limit` eager variance passes, materializing derived
    /// enodes and order edges. This does not apply rewrite rules.
    pub fn refinement_closure(&mut self, limit: usize) {
        let previous = self.variance_strategy;
        self.variance_strategy = VarianceStrategy::Eager;
        for _ in 0..limit {
            if !self.rewrite_step_with_order(&[], &[], &[]) {
                break;
            }
        }
        self.variance_strategy = previous;
    }

    pub fn variance_profile(&self) -> VarianceProfile {
        self.variance_profile
    }

    /// Experimental order index that materializes known <= pairs during rebuild.
    pub fn with_materialized_order() -> Self {
        Self {
            uf: UnionFind::with_materialized_closure(),
            ..Self::default()
        }
    }

    pub fn equivalent(&self, a: Id, b: Id) -> bool {
        self.uf.are_eq(a, b)
    }
    pub fn assert_le(&mut self, a: Id, b: Id) -> bool {
        let changed = self.uf.assert_le(a, b);
        self.needs_rebuild |= changed;
        changed
    }
    pub fn is_le(&self, a: Id, b: Id) -> bool {
        self.uf.is_le(a, b)
    }
    pub fn upper_classes(&self, id: Id) -> Vec<Id> {
        self.uf.upper_set(id)
    }
    pub fn lower_classes(&self, id: Id) -> Vec<Id> {
        self.uf.lower_set(id)
    }
    fn classes_in_mode(&self, id: Id, mode: Mode) -> Vec<Id> {
        match mode {
            Mode::Below => self.lower_classes(id),
            Mode::Exact => vec![self.uf.find(id)],
            Mode::Above => self.upper_classes(id),
        }
    }
    pub fn declare_variance(&mut self, symbol: Symbol, args: Vec<Variance>) -> Result<(), String> {
        if let Some(old) = self.variance.get(&symbol) {
            return if old == &args {
                Ok(())
            } else {
                Err(format!("conflicting variance declaration for {symbol}"))
            };
        }
        if self
            .nodes
            .keys()
            .any(|node| node.f == symbol && node.args.len() != args.len())
        {
            return Err(format!("existing {symbol} node has a different arity"));
        }
        self.variance.insert(symbol, args);
        Ok(())
    }
    pub fn statistics(&self) -> (usize, usize) {
        (self.rev.len(), self.nodes.len())
    }
    pub fn match_count(&self, pattern: &Sexp) -> usize {
        self.rev
            .keys()
            .map(|&class| self.ematch(pattern, class).len())
            .sum()
    }
    pub fn dump(&self) -> String {
        self.rev
            .iter()
            .map(|(id, nodes)| format!("{id}: {}", DisplayIter(nodes, ", ")))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Run one rewrite round, returning whether any nodes or classes changed.
    pub fn rewrite_step(&mut self, rewrites: &[Rewrite]) -> bool {
        self.rebuild();
        let before = (self.nodes.len(), self.uf.n_classes());
        self.rewrite(rewrites);
        self.rebuild();
        before != (self.nodes.len(), self.uf.n_classes())
    }

    fn propagate_variance(&mut self) -> bool {
        if self.variance.is_empty() {
            return false;
        }
        let start = Instant::now();
        let mut profile = VarianceProfile::default();
        let changed = match self.variance_strategy {
            VarianceStrategy::Eager => self.propagate_variance_eager(&mut profile),
            VarianceStrategy::Cartesian => self.propagate_variance_cartesian(&mut profile),
            VarianceStrategy::Pairwise => self.propagate_variance_pairwise(&mut profile),
        };
        self.variance_profile.time += start.elapsed();
        self.variance_profile.candidates += profile.candidates;
        self.variance_profile.edges_added += profile.edges_added;
        changed
    }

    fn propagate_variance_eager(&mut self, profile: &mut VarianceProfile) -> bool {
        let before_nodes = self.nodes.len();
        let nodes: Vec<_> = self
            .rev
            .iter()
            .flat_map(|(&class, nodes)| nodes.iter().map(move |node| (class, node.clone())))
            .filter(|(_, node)| {
                self.variance
                    .get(&node.f)
                    .is_some_and(|v| v.len() == node.args.len())
            })
            .collect();
        let mut new_order = false;
        for (class, node) in nodes {
            for (index, variance) in self.variance[&node.f].clone().into_iter().enumerate() {
                if variance == Variance::Invariant {
                    continue;
                }
                let child = self.uf.find(node.args[index]);
                for lower in self.uf.direct_lower(child) {
                    profile.candidates += 1;
                    let added =
                        self.variance_neighbor(class, &node, index, variance, lower, Mode::Below);
                    profile.edges_added += u64::from(added);
                    new_order |= added;
                }
                for upper in self.uf.direct_upper(child) {
                    profile.candidates += 1;
                    let added =
                        self.variance_neighbor(class, &node, index, variance, upper, Mode::Above);
                    profile.edges_added += u64::from(added);
                    new_order |= added;
                }
            }
        }

        new_order || self.nodes.len() != before_nodes
    }

    fn add_existing_order_edge(
        &mut self,
        lower: Id,
        upper: Id,
        profile: &mut VarianceProfile,
    ) -> bool {
        let added = self.uf.assert_le_edge(lower, upper);
        self.needs_rebuild |= added;
        profile.edges_added += u64::from(added);
        added
    }

    fn propagate_variance_pairwise(&mut self, profile: &mut VarianceProfile) -> bool {
        let mut groups: FxHashMap<Symbol, Vec<(Id, Node)>> = FxHashMap::default();
        for (node, &class) in &self.nodes {
            if self
                .variance
                .get(&node.f)
                .is_some_and(|v| v.len() == node.args.len())
            {
                groups
                    .entry(node.f)
                    .or_default()
                    .push((class, node.clone()));
            }
        }
        let mut changed = false;
        for (symbol, nodes) in groups {
            let signature = self.variance[&symbol].clone();
            for (lower_class, lower) in &nodes {
                for (upper_class, upper) in &nodes {
                    if self.uf.are_eq(*lower_class, *upper_class) {
                        continue;
                    }
                    profile.candidates += 1;
                    let ordered = signature.iter().enumerate().all(|(index, variance)| {
                        variance.act(Mode::Above).holds(
                            &self.uf,
                            lower.args[index],
                            upper.args[index],
                        )
                    });
                    if ordered {
                        changed |=
                            self.add_existing_order_edge(*lower_class, *upper_class, profile);
                    }
                }
            }
        }
        changed
    }

    fn propagate_variance_cartesian(&mut self, profile: &mut VarianceProfile) -> bool {
        let nodes: Vec<_> = self
            .nodes
            .iter()
            .filter(|(node, _)| {
                self.variance
                    .get(&node.f)
                    .is_some_and(|v| v.len() == node.args.len())
            })
            .map(|(node, &class)| (class, node.clone()))
            .collect();
        let mut uppers = FxHashMap::default();
        let mut lowers = FxHashMap::default();
        let mut changed = false;
        for (class, node) in nodes {
            let mut choices = Vec::with_capacity(node.args.len());
            for (child, variance) in node.args.iter().zip(&self.variance[&node.f]) {
                let child = self.uf.find(*child);
                let options = match variance.act(Mode::Above) {
                    Mode::Above => uppers
                        .entry(child)
                        .or_insert_with(|| self.uf.upper_set(child))
                        .clone(),
                    Mode::Below => lowers
                        .entry(child)
                        .or_insert_with(|| self.uf.lower_set(child))
                        .clone(),
                    Mode::Exact => vec![child],
                };
                choices.push(options);
            }
            let mut args = node.args.clone();
            changed |= self.lookup_variance_product(class, node.f, &choices, 0, &mut args, profile);
        }
        changed
    }

    fn lookup_variance_product(
        &mut self,
        source: Id,
        symbol: Symbol,
        choices: &[Vec<Id>],
        index: usize,
        args: &mut Vec<Id>,
        profile: &mut VarianceProfile,
    ) -> bool {
        if index == choices.len() {
            profile.candidates += 1;
            if let Some(&target) = self.nodes.get(&Node {
                f: symbol,
                args: args.clone(),
            }) {
                return self.add_existing_order_edge(source, target, profile);
            }
            return false;
        }
        let mut changed = false;
        for &child in &choices[index] {
            args[index] = child;
            changed |=
                self.lookup_variance_product(source, symbol, choices, index + 1, args, profile);
        }
        changed
    }

    fn variance_neighbor(
        &mut self,
        class: Id,
        node: &Node,
        index: usize,
        variance: Variance,
        child: Id,
        child_mode: Mode,
    ) -> bool {
        let mut variant = node.clone();
        variant.args[index] = child;
        let other = self.add_node(variant);
        let changed = match variance.act(child_mode) {
            Mode::Above => self.uf.assert_le_edge(class, other),
            Mode::Below => self.uf.assert_le_edge(other, class),
            Mode::Exact => unreachable!(),
        };
        self.needs_rebuild |= changed;
        changed
    }

    /// Pick the smallest finite term in a class. Cycles are skipped.
    pub fn extract(&self, id: Id) -> Option<Sexp> {
        self.extract_in_mode(id, Mode::Exact)
    }

    /// Pick the smallest term from any class known to be <= this class.
    pub fn extract_le(&self, id: Id) -> Option<Sexp> {
        self.extract_in_mode(id, Mode::Below)
    }

    /// Pick the smallest term from any class known to be >= this class.
    pub fn extract_ge(&self, id: Id) -> Option<Sexp> {
        self.extract_in_mode(id, Mode::Above)
    }

    fn extract_in_mode(&self, id: Id, mode: Mode) -> Option<Sexp> {
        let best = self.best_terms();
        self.classes_in_mode(id, mode)
            .iter()
            .filter_map(|class| best.get(class))
            .min_by_key(|(cost, _)| *cost)
            .map(|(_, term)| term.clone())
    }

    fn best_terms(&self) -> IndexMap<Id, (usize, Sexp)> {
        let mut best: IndexMap<Id, (usize, Sexp)> = IndexMap::default();
        loop {
            let mut changed = false;
            for (&class, nodes) in &self.rev {
                for node in nodes {
                    let mut cost = 1usize;
                    let mut args = Vec::new();
                    let mut ready = true;
                    for child in &node.args {
                        if let Some((child_cost, term)) = best.get(&self.uf.find(*child)) {
                            cost = cost.saturating_add(*child_cost);
                            args.push(term.clone());
                        } else {
                            ready = false;
                            break;
                        }
                    }
                    if !ready || best.get(&class).is_some_and(|(old, _)| *old <= cost) {
                        continue;
                    }
                    let term = if args.is_empty() {
                        Sexp::Atom(node.f)
                    } else {
                        let mut items = vec![Sexp::Atom(node.f)];
                        items.extend(args);
                        Sexp::List(items)
                    };
                    best.insert(class, (cost, term));
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        best
    }

    pub fn add_node(&mut self, node: Node) -> Id {
        let node = self.canonicalize_node(&node);
        if let Some(&id) = self.nodes.get(&node) {
            return self.uf.find_mut(id);
        }
        let id = self.uf.mkset();
        self.nodes.insert(node, id);
        self.needs_rebuild = true;
        id
    }

    /// Parse and add an s-exp in one step, useful for testing
    pub fn add(&mut self, s: &str) -> Id {
        self.instantiate(&s.parse().unwrap(), &Subst::default())
    }

    pub fn union(&mut self, a: Id, b: Id) -> bool {
        let changed = self.uf.union(a, b);
        self.needs_rebuild |= changed;
        changed
    }

    pub fn nodes_in_class(&self, class: Id) -> impl Iterator<Item = &Node> {
        let class = self.uf.find(class);
        self.rev.get(&class).into_iter().flatten()
    }

    fn is_node_canonical(&self, node: &Node) -> bool {
        node.args.iter().all(|id| self.uf.is_leader(*id))
    }

    pub fn canonicalize_node(&mut self, node: &Node) -> Node {
        Node {
            f: node.f,
            args: node.args.iter().map(|id| self.uf.find_mut(*id)).collect(),
        }
    }

    pub fn rebuild(&mut self) {
        if !self.needs_rebuild {
            return;
        }
        let mut keep_going = true;
        while keep_going {
            keep_going = false;
            let nodes = std::mem::take(&mut self.nodes);
            for (node, id) in nodes {
                let node = self.canonicalize_node(&node);
                let id = self.uf.find_mut(id);
                let id2 = *self.nodes.entry(node).or_insert(id);
                if self.union(id, id2) {
                    keep_going = true;
                }
            }
            if self.uf.collapse_order_cycle() {
                keep_going = true;
            }
        }
        self.uf.rebuild_closure();

        // rebuild the reverse map from scratch
        self.rev.clear();
        for (node, id) in &self.nodes {
            self.rev.entry(*id).or_default().push(node.clone());
        }
        self.rev.sort_keys();
        self.needs_rebuild = false;

        if cfg!(debug_assertions) {
            // nodes in nodes are canonical
            for (node, id) in &self.nodes {
                assert!(self.uf.is_leader(*id));
                assert!(self.is_node_canonical(node));
            }

            // nodes in class map are canonical
            for (id, nodes) in &self.rev {
                assert!(self.uf.is_leader(*id));
                for node in nodes {
                    assert!(self.is_node_canonical(node));
                }
            }

            // class map has exactly the leaders
            for i in 0..self.uf.n_classes() {
                let id = Id::new(i);
                if self.uf.is_leader(id) {
                    assert!(self.rev.contains_key(&id));
                } else {
                    assert!(!self.rev.contains_key(&id));
                }
            }
        }
    }

    pub fn print_statistics(&self) {
        let n_classes = self.rev.len();
        let n_nodes = self.nodes.len();
        println!("classes: {n_classes}, nodes: {n_nodes}");
    }

    pub fn print(&self) {
        for (id, nodes) in &self.rev {
            println!("{id}: {}", DisplayIter(nodes, ", "));
        }
    }
}

// e-matching
// we will reuse sexps as patterns, where atoms starting with ? are treated as pattern variables
impl EGraph {
    pub fn ematch(&self, pat: &Sexp, class: Id) -> Vec<Subst> {
        self.ematch_rec(pat, class, Default::default())
    }
    pub fn ematch_rec(&self, pat: &Sexp, class: Id, subst: Subst) -> Vec<Subst> {
        self.ematch_direction(pat, class, subst, Mode::Exact)
    }

    /// Find substitutions for which `class <= pat[subst]`.
    pub fn ematch_above(&self, pat: &Sexp, class: Id) -> Vec<Subst> {
        self.ematch_direction(pat, class, Default::default(), Mode::Above)
    }

    /// Find substitutions for which `pat[subst] <= class`.
    pub fn ematch_below(&self, pat: &Sexp, class: Id) -> Vec<Subst> {
        self.ematch_direction(pat, class, Default::default(), Mode::Below)
    }

    fn ematch_direction(&self, pat: &Sexp, class: Id, subst: Subst, direction: Mode) -> Vec<Subst> {
        self.classes_in_mode(class, direction)
            .into_iter()
            .flat_map(|candidate| self.ematch_in_class(pat, candidate, subst.clone(), direction))
            .collect()
    }

    fn ematch_in_class(&self, pat: &Sexp, class: Id, subst: Subst, direction: Mode) -> Vec<Subst> {
        match pat {
            Sexp::Atom(name) => {
                // all atoms beginning with ? are treated as pattern variables
                let subst: Option<Subst> = if name.as_str().starts_with('?') {
                    subst.with(*name, class)
                } else {
                    let leaf = Node {
                        f: *name,
                        args: vec![],
                    };
                    (self.nodes.get(&leaf) == Some(&class)).then(|| subst)
                };
                subst.into_iter().collect()
            }
            Sexp::List(items) => {
                let Some((Sexp::Atom(f), args)) = items.split_first() else {
                    panic!("expected atom at head of list");
                };
                self.nodes_in_class(class)
                    .filter(|node| (node.f, node.args.len()) == (*f, args.len()))
                    .flat_map(|node| {
                        let init = vec![subst.clone()];
                        args.iter().zip(&node.args).enumerate().fold(
                            init,
                            |todo, (i, (pa, &na))| {
                                let variance = self
                                    .variance
                                    .get(f)
                                    .filter(|signature| signature.len() == args.len())
                                    .map(|signature| signature[i])
                                    .unwrap_or(Variance::Invariant);
                                let child_direction = variance.act(direction);
                                let rec =
                                    |subst| self.ematch_direction(pa, na, subst, child_direction);
                                todo.into_iter().flat_map(rec).collect()
                            },
                        )
                    })
                    .collect()
            }
        }
    }
}

pub type Rewrite = (Sexp, Sexp);

#[derive(Clone, Debug)]
pub enum RuleFact {
    Eq(Sexp, Sexp),
    Le(Sexp, Sexp),
    Ge(Sexp, Sexp),
    Rel(Sexp),
}

#[derive(Clone, Debug)]
pub enum RuleConclusion {
    Eq(Sexp, Sexp),
    Le(Sexp, Sexp),
    Ge(Sexp, Sexp),
}

#[derive(Clone, Debug)]
pub struct Rule {
    pub premises: Vec<RuleFact>,
    pub conclusion: RuleConclusion,
}

// rewriting, rebuilding
impl EGraph {
    /// Apply equality, `<=`, and `>=` rewrites to the same graph snapshot.
    pub fn rewrite_step_with_order(
        &mut self,
        rewrites: &[Rewrite],
        le_rewrites: &[Rewrite],
        ge_rewrites: &[Rewrite],
    ) -> bool {
        self.rewrite_step_with_rules(rewrites, le_rewrites, ge_rewrites, &[])
    }

    /// Apply rewrites and multipattern rules against one rebuilt graph snapshot.
    pub fn rewrite_step_with_rules(
        &mut self,
        rewrites: &[Rewrite],
        le_rewrites: &[Rewrite],
        ge_rewrites: &[Rewrite],
        rules: &[Rule],
    ) -> bool {
        self.rebuild();
        let before = (self.nodes.len(), self.uf.n_classes());
        let equalities = self.collect_matches(rewrites);
        let less_than = self.collect_order_matches(le_rewrites, Mode::Above);
        let greater_than = self.collect_order_matches(ge_rewrites, Mode::Below);
        let rule_matches = self.collect_rule_matches(rules);
        let mut unions = self.apply_matches(equalities);
        let mut new_order = false;
        for (rhs, class, matches) in less_than {
            for subst in matches {
                let upper = self.instantiate(rhs, &subst);
                new_order |= self.assert_le(class, upper);
            }
        }
        for (rhs, class, matches) in greater_than {
            for subst in matches {
                let lower = self.instantiate(rhs, &subst);
                new_order |= self.assert_le(lower, class);
            }
        }
        for (rule, matches) in rule_matches {
            for subst in matches {
                match &rule.conclusion {
                    RuleConclusion::Eq(left, right) => {
                        let left = self.instantiate(left, &subst);
                        let right = self.instantiate(right, &subst);
                        unions += usize::from(self.union(left, right));
                    }
                    RuleConclusion::Le(left, right) => {
                        let left = self.instantiate(left, &subst);
                        let right = self.instantiate(right, &subst);
                        new_order |= self.assert_le(left, right);
                    }
                    RuleConclusion::Ge(left, right) => {
                        let left = self.instantiate(left, &subst);
                        let right = self.instantiate(right, &subst);
                        new_order |= self.assert_le(right, left);
                    }
                }
            }
        }
        if unions != 0 || new_order || self.nodes.len() != before.0 {
            self.rebuild();
        }
        let variance_change = self.propagate_variance();
        if variance_change {
            self.rebuild();
        }
        new_order || variance_change || before != (self.nodes.len(), self.uf.n_classes())
    }

    /// Apply equality rewrites and `<=` rewrites.
    pub fn refinement_step(&mut self, rewrites: &[Rewrite], refinements: &[Rewrite]) -> bool {
        self.rewrite_step_with_order(rewrites, refinements, &[])
    }

    pub fn instantiate(&mut self, pattern: &Sexp, subst: &Subst) -> Id {
        match pattern {
            Sexp::Atom(name) if name.as_str().starts_with('?') => subst[*name],
            Sexp::Atom(name) => self.add_node(Node {
                f: *name,
                args: vec![],
            }),
            Sexp::List(items) => {
                let Some((Sexp::Atom(f), args)) = items.split_first() else {
                    panic!("expected atom at head of list");
                };
                let rec = |arg| self.instantiate(arg, subst);
                let args = args.iter().map(rec).collect();
                self.add_node(Node { f: *f, args })
            }
        }
    }

    pub fn rewrite(&mut self, rewrites: &[Rewrite]) {
        let matches = self.collect_matches(rewrites);
        self.apply_matches(matches);
    }

    fn collect_matches<'a>(&self, rewrites: &'a [Rewrite]) -> Vec<(&'a Sexp, Id, Vec<Subst>)> {
        let mut all_matches = vec![];
        for rw in rewrites {
            for &class in self.rev.keys() {
                let matches = self.ematch(&rw.0, class);
                if !matches.is_empty() {
                    all_matches.push((&rw.1, class, matches));
                }
            }
        }
        all_matches
    }

    fn collect_order_matches<'a>(
        &self,
        rules: &'a [Rewrite],
        direction: Mode,
    ) -> Vec<(&'a Sexp, Id, Vec<Subst>)> {
        let mut all_matches = vec![];
        for (lhs, rhs) in rules {
            for &class in self.rev.keys() {
                let matches = self.ematch_direction(lhs, class, Default::default(), direction);
                if !matches.is_empty() {
                    all_matches.push((rhs, class, matches));
                }
            }
        }
        all_matches
    }

    fn collect_rule_matches<'a>(&self, rules: &'a [Rule]) -> Vec<(&'a Rule, Vec<Subst>)> {
        rules
            .iter()
            .map(|rule| {
                let matches = rule
                    .premises
                    .iter()
                    .fold(vec![Subst::default()], |todo, fact| {
                        todo.into_iter()
                            .flat_map(|subst| self.match_rule_fact(fact, subst))
                            .collect()
                    });
                (rule, matches)
            })
            .collect()
    }

    fn match_rule_fact(&self, fact: &RuleFact, subst: Subst) -> Vec<Subst> {
        match fact {
            RuleFact::Rel(pattern) => self
                .rev
                .keys()
                .flat_map(|&class| self.ematch_rec(pattern, class, subst.clone()))
                .collect(),
            RuleFact::Eq(left, right) | RuleFact::Le(left, right) | RuleFact::Ge(left, right) => {
                let mut matches = Vec::new();
                for &left_class in self.rev.keys() {
                    for left_subst in self.ematch_rec(left, left_class, subst.clone()) {
                        let mode = match fact {
                            RuleFact::Eq(_, _) => Mode::Exact,
                            RuleFact::Le(_, _) => Mode::Above,
                            RuleFact::Ge(_, _) => Mode::Below,
                            RuleFact::Rel(_) => unreachable!(),
                        };
                        for right_class in self.classes_in_mode(left_class, mode) {
                            matches.extend(self.ematch_rec(right, right_class, left_subst.clone()));
                        }
                    }
                }
                matches
            }
        }
    }

    fn apply_matches(&mut self, all_matches: Vec<(&Sexp, Id, Vec<Subst>)>) -> usize {
        let mut unions = 0;
        for (rhs, class, matches) in all_matches {
            for subst in matches {
                let replacement = self.instantiate(rhs, &subst);
                unions += usize::from(self.union(class, replacement));
            }
        }
        unions
    }

    /// Profile a round with the same match/apply/rebuild sequence as `rewrite_to_fixed`.
    /// Call `rebuild` before the first round.
    pub fn profile_step(&mut self, rewrites: &[Rewrite]) -> StepProfile {
        let before = (self.nodes.len(), self.uf.n_classes());
        let start = Instant::now();
        let matches = self.collect_matches(rewrites);
        let matching = start.elapsed();
        let match_count = matches
            .iter()
            .map(|(_, _, substitutions)| substitutions.len())
            .sum();
        let start = Instant::now();
        let unions = self.apply_matches(matches);
        let applying = start.elapsed();
        let new_nodes = self.nodes.len() - before.0;
        let start = Instant::now();
        self.rebuild();
        let rebuilding = start.elapsed();
        StepProfile {
            matching,
            applying,
            rebuilding,
            matches: match_count,
            new_nodes,
            unions,
            changed: before != (self.nodes.len(), self.uf.n_classes()),
        }
    }

    pub fn rewrite_to_fixed(&mut self, rewrites: &[Rewrite]) {
        self.rebuild();
        loop {
            let before = (self.nodes.len(), self.uf.n_classes());
            self.rewrite(rewrites);
            self.rebuild();
            let after = (self.nodes.len(), self.uf.n_classes());
            if before == after {
                break;
            }
        }
    }
}

#[test]
fn test_rebuild() {
    let mut eg = EGraph::default();
    let b = eg.add("b");
    let c = eg.add("c");
    let f1 = eg.add("(f a b)");
    let f2 = eg.add("(f a c)");

    eg.union(b, c);
    assert!(!eg.uf.are_eq(f1, f2));

    eg.rebuild();
    assert!(eg.uf.are_eq(f1, f2));
}

#[test]
fn test_match() {
    let mut eg = EGraph::default();
    let f1 = eg.add("(f a a)");
    let f2 = eg.add("(f b b)");
    let f3 = eg.add("(f a b)");

    eg.union(f1, f2);
    eg.union(f2, f3);
    eg.rebuild();

    let fxx_matches = eg.ematch(&sexp("(f ?x ?x)"), f1);
    assert_eq!(fxx_matches.len(), 2);

    let fxy_matches = eg.ematch(&sexp("(f ?x ?y)"), f1);
    assert_eq!(fxy_matches.len(), 3);
}

#[test]
fn test_ac_rewriting() {
    let mut eg = EGraph::default();

    let n = var_parse_or("n", 7);
    let atoms = (0..n).map(|i| format!("x{}", i));
    let f = |acc, x| format!("(f {acc} {x})");
    let input = atoms.clone().reduce(f).unwrap();
    let goal = atoms.rev().reduce(f).unwrap();

    let input = eg.add(&input);
    let goal = eg.add(&goal);
    let rws = [
        (sexp("(f (f ?x ?y) ?z)"), sexp("(f ?x (f ?y ?z))")),
        (sexp("(f ?x ?y)"), sexp("(f ?y ?x)")),
    ];

    eg.rewrite_to_fixed(&rws);
    eg.print_statistics();

    assert!(eg.uf.are_eq(input, goal));

    assert_eq!(eg.uf.n_classes(), 2usize.pow(n as _) - 1);
}

#[test]
fn existing_node_variance_closes_all_arguments_without_new_enodes() {
    for strategy in [VarianceStrategy::Cartesian, VarianceStrategy::Pairwise] {
        let mut eg = EGraph::default();
        eg.set_variance_strategy(strategy);
        eg.declare_variance("f".into(), vec![Variance::Covariant, Variance::Covariant])
            .unwrap();
        let a = eg.add("a");
        let b = eg.add("b");
        eg.assert_le(a, b);
        let lower = eg.add("(f (f a a) a)");
        let upper = eg.add("(f (f b b) b)");
        for _ in 0..5 {
            if !eg.refinement_step(&[], &[]) {
                break;
            }
        }
        assert!(eg.is_le(lower, upper), "{strategy:?}");
        assert_eq!(eg.statistics(), (6, 6), "{strategy:?}");
    }
}

#[test]
fn existing_node_variance_respects_contravariance() {
    for strategy in [VarianceStrategy::Cartesian, VarianceStrategy::Pairwise] {
        let mut eg = EGraph::default();
        eg.set_variance_strategy(strategy);
        eg.declare_variance(
            "f".into(),
            vec![Variance::Covariant, Variance::Contravariant],
        )
        .unwrap();
        let a = eg.add("a");
        let b = eg.add("b");
        eg.assert_le(a, b);
        let lower = eg.add("(f a b)");
        let upper = eg.add("(f b a)");
        for _ in 0..3 {
            if !eg.refinement_step(&[], &[]) {
                break;
            }
        }
        assert!(eg.is_le(lower, upper), "{strategy:?}");
        assert!(!eg.is_le(upper, lower), "{strategy:?}");
        assert_eq!(eg.statistics(), (4, 4), "{strategy:?}");
    }
}

#[test]
fn refinement_matches_a_covariant_child_without_materializing_it() {
    let mut eg = EGraph::default();
    eg.set_variance_strategy(VarianceStrategy::Pairwise);
    eg.declare_variance("f".into(), vec![Variance::Covariant])
        .unwrap();
    let a = eg.add("a");
    let b = eg.add("b");
    eg.assert_le(a, b);
    let target = eg.add("(f a)");
    let refinements = [(sexp("(f b)"), sexp("z"))];

    eg.refinement_step(&[], &refinements);

    let z = eg.nodes[&Node {
        f: "z".into(),
        args: vec![],
    }];
    assert!(eg.is_le(target, z));
    assert!(!eg.nodes.contains_key(&Node {
        f: "f".into(),
        args: vec![b],
    }));
    assert!(eg.ematch_below(&sexp("(f b)"), target).is_empty());
}

#[test]
fn refinement_matching_flips_in_a_contravariant_child() {
    let mut eg = EGraph::default();
    eg.set_variance_strategy(VarianceStrategy::Pairwise);
    eg.declare_variance("f".into(), vec![Variance::Contravariant])
        .unwrap();
    let a = eg.add("a");
    let b = eg.add("b");
    eg.assert_le(a, b);
    let target = eg.add("(f b)");
    let refinements = [(sexp("(f a)"), sexp("z"))];

    eg.refinement_step(&[], &refinements);

    let z = eg.nodes[&Node {
        f: "z".into(),
        args: vec![],
    }];
    assert!(eg.is_le(target, z));
    assert!(!eg.nodes.contains_key(&Node {
        f: "f".into(),
        args: vec![a],
    }));
}

#[test]
fn refinement_matching_requires_equality_in_an_invariant_child() {
    let mut eg = EGraph::default();
    eg.set_variance_strategy(VarianceStrategy::Pairwise);
    eg.declare_variance("f".into(), vec![Variance::Invariant])
        .unwrap();
    let a = eg.add("a");
    let b = eg.add("b");
    eg.assert_le(a, b);
    let target = eg.add("(f a)");
    let refinements = [(sexp("(f b)"), sexp("z"))];

    eg.refinement_step(&[], &refinements);
    assert!(!eg.nodes.contains_key(&Node {
        f: "z".into(),
        args: vec![],
    }));

    eg.union(a, b);
    eg.refinement_step(&[], &refinements);
    let z = eg.nodes[&Node {
        f: "z".into(),
        args: vec![],
    }];
    assert!(eg.is_le(target, z));
}

#[test]
fn refinement_matching_searches_upper_eclasses() {
    let mut eg = EGraph::default();
    eg.set_variance_strategy(VarianceStrategy::Pairwise);
    let a = eg.add("a");
    let b = eg.add("b");
    eg.assert_le(a, b);

    assert_eq!(eg.ematch_below(&sexp("a"), b).len(), 1);

    eg.refinement_step(&[], &[(sexp("b"), sexp("z"))]);

    let z = eg.nodes[&Node {
        f: "z".into(),
        args: vec![],
    }];
    assert!(eg.is_le(a, z));
}

#[test]
fn existing_node_variance_does_not_match_unmaterialized_rewrite_terms() {
    let run = |strategy| {
        let mut eg = EGraph::default();
        eg.set_variance_strategy(strategy);
        eg.declare_variance("f".into(), vec![Variance::Covariant])
            .unwrap();
        let a = eg.add("a");
        let b = eg.add("b");
        eg.assert_le(a, b);
        eg.add("(f a)");
        let rewrites = [(sexp("(f b)"), sexp("c"))];
        for _ in 0..5 {
            if !eg.refinement_step(&rewrites, &[]) {
                break;
            }
        }
        eg.nodes.contains_key(&Node {
            f: "c".into(),
            args: vec![],
        })
    };
    assert!(run(VarianceStrategy::Eager));
    assert!(!run(VarianceStrategy::Cartesian));
    assert!(!run(VarianceStrategy::Pairwise));
}

#[test]
fn variance_strategies_agree_on_existing_ac_terms() {
    fn saturated(strategy: VarianceStrategy) -> EGraph {
        let mut eg = EGraph::default();
        eg.set_variance_strategy(strategy);
        let atoms: Vec<_> = (0..=4).map(|i| format!("x{i}")).collect();
        let fold = |terms: &[String]| {
            terms
                .iter()
                .cloned()
                .reduce(|left, right| format!("(U {left} {right})"))
                .unwrap()
        };
        eg.add(&fold(&atoms[..4]));
        eg.declare_variance("U".into(), vec![Variance::Covariant; 2])
            .unwrap();
        for pair in atoms.windows(2) {
            let lower = eg.add(&pair[0]);
            let upper = eg.add(&pair[1]);
            eg.assert_le(lower, upper);
        }
        let mut goal = atoms[1..].to_vec();
        goal.reverse();
        eg.add(&fold(&goal));
        let rewrites = [
            (sexp("(U (U ?x ?y) ?z)"), sexp("(U ?x (U ?y ?z))")),
            (sexp("(U ?x ?y)"), sexp("(U ?y ?x)")),
        ];
        for _ in 0..30 {
            if !eg.refinement_step(&rewrites, &[]) {
                return eg;
            }
        }
        panic!("strategy did not saturate: {strategy:?}");
    }
    let eager = saturated(VarianceStrategy::Eager);
    let cartesian = saturated(VarianceStrategy::Cartesian);
    let pairwise = saturated(VarianceStrategy::Pairwise);
    fn signature(term: &Sexp, leaves: &mut Vec<String>) {
        match term {
            Sexp::Atom(atom) => leaves.push(atom.to_string()),
            Sexp::List(items) => {
                for arg in &items[1..] {
                    signature(arg, leaves);
                }
            }
        }
    }
    fn classes(eg: &EGraph) -> std::collections::BTreeMap<Vec<String>, Id> {
        eg.best_terms()
            .into_iter()
            .map(|(class, (_, term))| {
                let mut leaves = Vec::new();
                signature(&term, &mut leaves);
                leaves.sort();
                (leaves, class)
            })
            .collect()
    }
    let eager_classes = classes(&eager);
    let cartesian_classes = classes(&cartesian);
    let pairwise_classes = classes(&pairwise);
    assert_eq!(
        cartesian_classes.keys().collect::<Vec<_>>(),
        pairwise_classes.keys().collect::<Vec<_>>()
    );
    for (left, &cart_left) in &cartesian_classes {
        let eager_left = eager_classes[left];
        let pair_left = pairwise_classes[left];
        for (right, &cart_right) in &cartesian_classes {
            let expected = eager.is_le(eager_left, eager_classes[right]);
            assert_eq!(
                cartesian.is_le(cart_left, cart_right),
                expected,
                "cartesian disagrees on {left:?} <= {right:?}"
            );
            assert_eq!(
                pairwise.is_le(pair_left, pairwise_classes[right]),
                expected,
                "pairwise disagrees on {left:?} <= {right:?}"
            );
        }
    }
}
