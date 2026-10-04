//! The non-binder subset of lambda-microegg's command language.
use crate::util::{Sexp, Symbol};
use crate::{EGraph, Rewrite, Rule, RuleConclusion, RuleFact, Variance, VarianceStrategy};

pub fn parse(input: &str) -> Result<Vec<Sexp>, String> {
    struct Parser<'a> {
        input: &'a str,
        pos: usize,
    }
    impl Parser<'_> {
        fn location(&self) -> String {
            let before = &self.input[..self.pos];
            format!(
                "line {}:{}",
                before.bytes().filter(|&b| b == b'\n').count() + 1,
                before.rsplit('\n').next().unwrap().chars().count() + 1
            )
        }
        fn error(&self, message: &str) -> String {
            format!("{}: {message}", self.location())
        }
        fn skip(&mut self) {
            loop {
                while self
                    .input
                    .as_bytes()
                    .get(self.pos)
                    .is_some_and(u8::is_ascii_whitespace)
                {
                    self.pos += 1;
                }
                if self.input.as_bytes().get(self.pos) == Some(&b';') {
                    while self
                        .input
                        .as_bytes()
                        .get(self.pos)
                        .is_some_and(|b| *b != b'\n')
                    {
                        self.pos += 1;
                    }
                } else {
                    break;
                }
            }
        }
        fn one(&mut self) -> Result<Sexp, String> {
            self.skip();
            match self.input.as_bytes().get(self.pos).copied() {
                Some(b'(' | b'[') => {
                    let open = self.input.as_bytes()[self.pos];
                    let close = if open == b'(' { b')' } else { b']' };
                    self.pos += 1;
                    let mut items = Vec::new();
                    loop {
                        self.skip();
                        match self.input.as_bytes().get(self.pos).copied() {
                            Some(b) if b == close => {
                                self.pos += 1;
                                break;
                            }
                            None => return Err(self.error("unclosed list")),
                            Some(b')' | b']') => {
                                return Err(self.error("mismatched closing delimiter"));
                            }
                            _ => items.push(self.one()?),
                        }
                    }
                    if open == b'[' {
                        if items.len() < 2 {
                            return Err(self.error("application needs an argument"));
                        }
                        let mut terms = items.into_iter();
                        let mut app = terms.next().unwrap();
                        for arg in terms {
                            app = Sexp::List(vec![Sexp::Atom("app".into()), app, arg]);
                        }
                        Ok(app)
                    } else {
                        Ok(Sexp::List(items))
                    }
                }
                Some(b')' | b']') => Err(self.error("unexpected closing delimiter")),
                Some(b'{' | b'}') => {
                    Err(self.error("metavariable occurrences require binder support"))
                }
                Some(b'"') => {
                    self.pos += 1;
                    let mut value = String::new();
                    loop {
                        match self.input.as_bytes().get(self.pos).copied() {
                            Some(b'"') => {
                                self.pos += 1;
                                break;
                            }
                            Some(b'\\') => {
                                self.pos += 1;
                                let ch = match self.input.as_bytes().get(self.pos).copied() {
                                    Some(b'n') => '\n',
                                    Some(b't') => '\t',
                                    Some(b'"') => '"',
                                    Some(b'\\') => '\\',
                                    _ => return Err(self.error("invalid string escape")),
                                };
                                self.pos += 1;
                                value.push(ch);
                            }
                            Some(_) => {
                                let ch = self.input[self.pos..].chars().next().unwrap();
                                self.pos += ch.len_utf8();
                                value.push(ch);
                            }
                            None => return Err(self.error("unclosed string")),
                        }
                    }
                    Ok(Sexp::Atom(value.into()))
                }
                Some(_) => {
                    let start = self.pos;
                    while let Some(&b) = self.input.as_bytes().get(self.pos) {
                        if b.is_ascii_whitespace() || b"()[]{};\"".contains(&b) {
                            break;
                        }
                        self.pos += 1;
                    }
                    if start == self.pos {
                        return Err(self.error("expected atom"));
                    }
                    Ok(Sexp::Atom(self.input[start..self.pos].into()))
                }
                None => Err(self.error("unexpected end of input")),
            }
        }
    }
    let mut parser = Parser { input, pos: 0 };
    let mut forms = Vec::new();
    parser.skip();
    while parser.pos < input.len() {
        forms.push(parser.one()?);
        parser.skip();
    }
    Ok(forms)
}

fn atom(form: &Sexp) -> Result<&str, String> {
    match form {
        Sexp::Atom(value) => Ok(value.as_str()),
        _ => Err("expected an atom".into()),
    }
}
fn term(form: &Sexp, pattern: bool) -> Result<(), String> {
    match form {
        Sexp::Atom(value) => {
            let name = value.as_str();
            if name.starts_with('@') || name.starts_with('$') || name == "#subst" {
                return Err(format!("'{name}' requires binder or lifting support"));
            }
            if name.starts_with('?') && !pattern {
                return Err(format!("pattern variable '{name}' in a term"));
            }
        }
        Sexp::List(items) => {
            let Some((head, args)) = items.split_first() else {
                return Err("empty term list".into());
            };
            let name = atom(head)?;
            if name.starts_with('@') || name == "#subst" {
                return Err(format!("'{name}' requires binder support"));
            }
            if name.starts_with('?') {
                return Err("pattern variable in function position".into());
            }
            for arg in args {
                term(arg, pattern)?;
            }
        }
    }
    Ok(())
}

fn variables(form: &Sexp, out: &mut Vec<Symbol>) {
    match form {
        Sexp::Atom(value) if value.as_str().starts_with('?') => {
            if !out.contains(value) {
                out.push(*value);
            }
        }
        Sexp::List(items) => {
            for item in items {
                variables(item, out);
            }
        }
        _ => {}
    }
}

fn rule_fact(form: &Sexp) -> Result<RuleFact, String> {
    if let Sexp::List(items) = form {
        if let Some(Sexp::Atom(head)) = items.first() {
            match head.as_str() {
                "=" | "<=" | ">=" => {
                    if items.len() != 3 {
                        return Err(format!("{head} expects two terms in a rule"));
                    }
                    term(&items[1], true)?;
                    term(&items[2], true)?;
                    return Ok(match head.as_str() {
                        "=" => RuleFact::Eq(items[1].clone(), items[2].clone()),
                        "<=" => RuleFact::Le(items[1].clone(), items[2].clone()),
                        ">=" => RuleFact::Ge(items[1].clone(), items[2].clone()),
                        _ => unreachable!(),
                    });
                }
                "Rel" => {
                    if items.len() != 2 {
                        return Err("Rel expects one term in a rule".into());
                    }
                    term(&items[1], true)?;
                    return Ok(RuleFact::Rel(items[1].clone()));
                }
                _ => {}
            }
        }
    }
    term(form, true)?;
    Ok(RuleFact::Rel(form.clone()))
}

pub fn run(input: &str) -> Result<Vec<String>, String> {
    let forms = parse(input)?;
    run_forms(&forms)
}

fn run_forms(forms: &[Sexp]) -> Result<Vec<String>, String> {
    let mut eg = EGraph::default();
    let mut rewrites: Vec<Rewrite> = Vec::new();
    let mut le_rewrites: Vec<Rewrite> = Vec::new();
    let mut ge_rewrites: Vec<Rewrite> = Vec::new();
    let mut rules: Vec<Rule> = Vec::new();
    let mut output = Vec::new();
    for (index, form) in forms.iter().enumerate() {
        let result: Result<(), String> = (|| {
            let Sexp::List(items) = form else {
                return Err("expected a command list".into());
            };
            let Some((name, args)) = items.split_first() else {
                return Err("empty command".into());
            };
            let name = atom(name)?;
            let arity = |n: usize| -> Result<(), String> {
                if args.len() == n {
                    Ok(())
                } else {
                    Err(format!(
                        "{name} expects {n} argument(s), got {}",
                        args.len()
                    ))
                }
            };
            match name {
                "reset" => {
                    arity(0)?;
                    eg = EGraph::default();
                    rewrites.clear();
                    le_rewrites.clear();
                    ge_rewrites.clear();
                    rules.clear();
                }
                "insert" => {
                    arity(1)?;
                    term(&args[0], false)?;
                    eg.instantiate(&args[0], &Default::default());
                }
                "union" | "guard" | "le" | "guard-le" => {
                    arity(2)?;
                    term(&args[0], false)?;
                    term(&args[1], false)?;
                    let a = eg.instantiate(&args[0], &Default::default());
                    let b = eg.instantiate(&args[1], &Default::default());
                    match name {
                        "union" => {
                            eg.union(a, b);
                            eg.rebuild();
                        }
                        "le" => {
                            eg.assert_le(a, b);
                            eg.rebuild();
                        }
                        "guard" => {
                            eg.rebuild();
                            if !eg.equivalent(a, b) {
                                return Err(format!("guard failed: {} != {}", args[0], args[1]));
                            }
                        }
                        "guard-le" => {
                            eg.rebuild();
                            if !eg.is_le(a, b) {
                                return Err(format!(
                                    "guard-le failed: {} <= {} is not known",
                                    args[0], args[1]
                                ));
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                "rewrite-le" | "rewrite-ge" => {
                    arity(2)?;
                    term(&args[0], true)?;
                    term(&args[1], true)?;
                    let mut bound = Vec::new();
                    let mut used = Vec::new();
                    variables(&args[0], &mut bound);
                    variables(&args[1], &mut used);
                    if used.iter().any(|variable| !bound.contains(variable)) {
                        return Err(format!("{name} RHS has an unbound pattern variable"));
                    }
                    let rules = if name == "rewrite-le" {
                        &mut le_rewrites
                    } else {
                        &mut ge_rewrites
                    };
                    rules.push((args[0].clone(), args[1].clone()));
                }
                "fun" => {
                    arity(2)?;
                    let symbol = atom(&args[0])?;
                    if symbol.starts_with('?') {
                        return Err("fun expects an operator name".into());
                    }
                    let Sexp::List(positions) = &args[1] else {
                        return Err("fun expects a variance list, such as (+ - =)".into());
                    };
                    let mut signature = Vec::new();
                    for position in positions {
                        signature.push(match atom(position)? {
                            "+" => Variance::Covariant,
                            "-" => Variance::Contravariant,
                            "=" => Variance::Invariant,
                            other => return Err(format!("unknown variance '{other}'")),
                        });
                    }
                    eg.declare_variance(symbol.into(), signature)?;
                }
                "rewrite" | "birewrite" => {
                    arity(2)?;
                    term(&args[0], true)?;
                    term(&args[1], true)?;
                    let mut lhs = Vec::new();
                    let mut rhs = Vec::new();
                    variables(&args[0], &mut lhs);
                    variables(&args[1], &mut rhs);
                    if rhs.iter().any(|v| !lhs.contains(v)) {
                        return Err("rewrite RHS has an unbound pattern variable".into());
                    }
                    if name == "birewrite" && lhs.iter().any(|v| !rhs.contains(v)) {
                        return Err("reverse rewrite RHS has an unbound pattern variable".into());
                    }
                    rewrites.push((args[0].clone(), args[1].clone()));
                    if name == "birewrite" {
                        rewrites.push((args[1].clone(), args[0].clone()));
                    }
                }
                "rule" => {
                    arity(2)?;
                    let Sexp::List(premises) = &args[0] else {
                        return Err("rule expects a list of premises".into());
                    };
                    let premises = premises
                        .iter()
                        .map(rule_fact)
                        .collect::<Result<Vec<_>, _>>()?;
                    let conclusion = match rule_fact(&args[1])? {
                        RuleFact::Eq(left, right) => RuleConclusion::Eq(left, right),
                        RuleFact::Le(left, right) => RuleConclusion::Le(left, right),
                        RuleFact::Ge(left, right) => RuleConclusion::Ge(left, right),
                        RuleFact::Rel(_) => {
                            return Err("rule conclusion must be =, <=, or >=".into());
                        }
                    };
                    let mut bound = Vec::new();
                    let mut used = Vec::new();
                    variables(&args[0], &mut bound);
                    variables(&args[1], &mut used);
                    if used.iter().any(|variable| !bound.contains(variable)) {
                        return Err("rule conclusion has an unbound pattern variable".into());
                    }
                    rules.push(Rule {
                        premises,
                        conclusion,
                    });
                }
                "run" => {
                    if !(1..=2).contains(&args.len()) {
                        return Err("run expects a limit and optional :expand-le".into());
                    }
                    let limit: usize = atom(&args[0])?
                        .parse()
                        .map_err(|_| "run expects a nonnegative integer".to_string())?;
                    if args.len() == 2 && atom(&args[1])? != ":expand-le" {
                        return Err("unknown run option (expected :expand-le)".into());
                    }
                    if args.len() == 2 {
                        eg.set_variance_strategy(VarianceStrategy::Eager);
                    }
                    for _ in 0..limit {
                        if !eg.rewrite_step_with_rules(
                            &rewrites,
                            &le_rewrites,
                            &ge_rewrites,
                            &rules,
                        ) {
                            break;
                        }
                    }
                    if args.len() == 2 {
                        eg.set_variance_strategy(VarianceStrategy::Cartesian);
                    }
                }
                "refinement-closure" => {
                    arity(1)?;
                    let limit: usize = atom(&args[0])?.parse().map_err(|_| {
                        "refinement-closure expects a nonnegative integer".to_string()
                    })?;
                    eg.refinement_closure(limit);
                }
                "match" => {
                    arity(1)?;
                    term(&args[0], true)?;
                    eg.rebuild();
                    output.push(format!("{} matches", eg.match_count(&args[0])));
                }
                "extract" | "extract-le" | "extract-ge" => {
                    arity(1)?;
                    term(&args[0], false)?;
                    let id = eg.instantiate(&args[0], &Default::default());
                    eg.rebuild();
                    let result = match name {
                        "extract" => eg.extract(id),
                        "extract-le" => eg.extract_le(id),
                        "extract-ge" => eg.extract_ge(id),
                        _ => unreachable!(),
                    };
                    output.push(
                        result
                            .ok_or("no finite term in eligible classes")?
                            .to_string(),
                    );
                }
                "echo" => {
                    arity(1)?;
                    output.push(atom(&args[0])?.to_string());
                }
                "print-egraph" => {
                    arity(0)?;
                    eg.rebuild();
                    output.push(eg.dump());
                }
                "fail" => {
                    arity(1)?;
                    let mut prefix = forms[..index].to_vec();
                    prefix.push(args[0].clone());
                    if run_forms(&prefix).is_ok() {
                        return Err("expected command to fail".into());
                    }
                }
                _ => return Err(format!("unknown command '{name}'")),
            }
            Ok(())
        })();
        result.map_err(|error| format!("command {}: {error}", index + 1))?;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_commands() {
        let lines = run(
            "(insert (+ a 0)) (rewrite (+ ?x 0) ?x) (run 3) (guard (+ a 0) a) (extract (+ a 0))",
        )
        .unwrap();
        assert_eq!(lines, ["a"]);
    }
    #[test]
    fn comments_strings_and_brackets() {
        assert_eq!(
            run("; hi\n(echo \"hi there\") (insert [f x]) (extract [f x])").unwrap(),
            ["hi there", "(app f x)"]
        );
    }
    #[test]
    fn reset_clears_order_rewrites() {
        run(
            "(rewrite-le A B) (rewrite-ge A C) (reset) (insert A) (run 2)
             (fail (guard-le A B)) (fail (guard-le C A))",
        )
        .unwrap();
    }
    #[test]
    fn reset_clears_multipattern_rules() {
        run("(rule ((Rel a)) (<= a b)) (reset) (insert a) (run 1)
             (fail (guard-le a b))")
        .unwrap();
    }
    #[test]
    fn refinement_is_directional_and_transitive() {
        run("(insert (inter A B))
             (rewrite-le (inter ?a ?b) ?a)
             (le A Top)
             (run 2)
             (guard-le (inter A B) A)
             (guard-le (inter A B) Top)
             (fail (guard-le A (inter A B)))")
        .unwrap();
    }
    #[test]
    fn refinement_constructs_its_right_hand_term() {
        run("(insert (f A))
             (rewrite-le (f ?x) (g ?x))
             (run 2)
             (guard-le (f A) (g A))
             (fail (guard (f A) (g A)))")
        .unwrap();
    }
    #[test]
    fn refinement_matches_upward_in_one_round() {
        run("(fun f (+))
             (le a b)
             (insert (f a))
             (rewrite-le (f b) z)
             (run 1)
             (guard-le (f a) z)")
        .unwrap();
    }
    #[test]
    fn rewrite_ge_matches_downward_in_one_round() {
        run("(fun f (+))
             (le a b)
             (insert (f b))
             (rewrite-ge (f a) z)
             (run 1)
             (guard-le z (f b))")
        .unwrap();
        run("(fun f (-))
             (le a b)
             (insert (f a))
             (rewrite-ge (f b) z)
             (run 1)
             (guard-le z (f a))")
        .unwrap();
    }
    #[test]
    fn rule_joins_relation_and_order_premises() {
        run("(insert (f a b)) (insert (f c d))
             (le a c) (le b d)
             (rule ((Rel (f ?x ?y)) (Rel (f ?u ?v))
                    (<= ?x ?u) (<= ?y ?v))
                   (<= (f ?x ?y) (f ?u ?v)))
             (run 1)
             (guard-le (f a b) (f c d))")
        .unwrap();
    }
    #[test]
    fn rule_supports_equality_and_bare_relation_premises() {
        run("(insert (pair a b)) (union a c)
             (rule ((pair ?x ?y) (= ?x c)) (= ?y d))
             (run 1)
             (guard b d)")
        .unwrap();
    }
    #[test]
    fn rel_variable_matches_any_existing_eclass() {
        run("(insert a) (insert b)
             (rule ((Rel ?x) (= ?x a)) (<= ?x Top))
             (run 1)
             (guard-le a Top)
             (fail (guard-le b Top))")
        .unwrap();
    }
    #[test]
    fn rule_supports_ge_premises_and_conclusions() {
        run("(insert (tag a)) (le a b)
             (rule ((Rel (tag ?x)) (>= b ?x)) (>= (tag ?x) z))
             (run 1)
             (guard-le z (tag a))")
        .unwrap();
    }
    #[test]
    fn rule_matching_does_not_panic_on_a_different_symbol_arity() {
        run("(fun f (+)) (insert (f a b))
             (rule ((Rel (f ?x ?y))) (<= ?x ?y))
             (run 1) (guard-le a b)")
        .unwrap();
    }
    #[test]
    fn rule_proves_residuation_from_an_inequality_premise() {
        run("(insert (comp J X)) (le (comp J X) G)
             (rule ((<= (comp ?j ?x) ?g))
                   (<= ?x (rdiv ?g ?j)))
             (run 1)
             (guard-le X (rdiv G J))")
        .unwrap();
    }
    #[test]
    fn dontcare_circuit_extracts_a_deterministic_refinement() {
        assert_eq!(
            run(include_str!("../examples/dontcare.sexp")).unwrap(),
            ["x"]
        );
    }
    #[test]
    fn extraction_follows_both_order_directions() {
        assert_eq!(
            run("(le a (f x)) (le (f x) b)
                 (extract (f x)) (extract-le (f x)) (extract-ge (f x))")
            .unwrap(),
            ["(f x)", "a", "b"]
        );
    }
    #[test]
    fn variance_signs_propagate_in_the_expected_directions() {
        run("(le a b)
             (fun cov (+)) (fun contra (-)) (fun inv (=))
             (insert (cov a)) (insert (contra a)) (insert (inv a))
             (refinement-closure 3)
             (guard-le (cov a) (cov b))
             (guard-le (contra b) (contra a))
             (fail (guard-le (inv a) (inv b)))")
        .unwrap();
    }
    #[test]
    fn direct_variance_edges_reach_transitive_nested_goal() {
        run("(le a b) (le b c)
             (fun f (+ +))
             (insert (f (f a a) a))
             (insert (f (f c c) c))
             (run 12)
             (guard-le (f (f a a) a) (f (f c c) c))")
        .unwrap();
    }
    #[test]
    fn variance_propagates_edges_added_after_an_earlier_run() {
        run("(fun f (+)) (insert (f a)) (run 2)
             (le a b) (refinement-closure 3) (guard-le (f a) (f b))")
        .unwrap();
    }
    #[test]
    fn variance_propagates_after_child_classes_merge() {
        run("(fun f (+)) (insert (f a)) (insert (f b)) (run 2)
             (union a b) (le b c) (refinement-closure 3) (guard-le (f a) (f c))")
        .unwrap();
    }
    #[test]
    fn explicit_refinement_closure_materializes_missing_enodes() {
        assert_eq!(
            run("(fun f (+)) (le a b) (insert (f a))
                 (rewrite (f b) z)
                 (run 2) (match (f b))
                 (refinement-closure 3) (match (f b)) (match z)
                 (run 1) (match z)
                 (le b c) (run 2) (match (f c))")
            .unwrap(),
            [
                "0 matches",
                "1 matches",
                "0 matches",
                "1 matches",
                "0 matches"
            ]
        );
    }
    #[test]
    fn expand_le_interleaves_variance_and_rewrites() {
        assert_eq!(
            run("(fun f (+)) (le a b) (insert (f a))
                 (rewrite (f b) z)
                 (run 1 :expand-le) (match (f b)) (match z)
                 (run 1) (match z)")
            .unwrap(),
            ["1 matches", "0 matches", "1 matches"]
        );
    }
    #[test]
    fn expand_le_only_applies_to_one_run() {
        assert_eq!(
            run("(fun f (+)) (le a b) (insert (f a))
                 (run 1 :expand-le)
                 (le b c) (run 2) (match (f c))
                 (run 1 :expand-le) (match (f c))")
            .unwrap(),
            ["0 matches", "1 matches"]
        );
    }
    #[test]
    fn expand_le_limit_stops_cyclic_growth() {
        assert_eq!(
            run("(fun f (+)) (union a (f a)) (le a b)
                 (run 3 :expand-le)
                 (match (f (f (f b))))
                 (match (f (f (f (f b)))))")
            .unwrap(),
            ["1 matches", "0 matches"]
        );
    }
    #[test]
    fn refinement_closure_limit_stops_cyclic_growth() {
        assert_eq!(
            run("(fun f (+)) (union a (f a)) (le a b)
                 (refinement-closure 3)
                 (match (f (f (f b))))
                 (match (f (f (f (f b)))))")
            .unwrap(),
            ["1 matches", "0 matches"]
        );
    }
    #[test]
    fn order_cycle_becomes_equality_and_rebuilds_congruence() {
        run("(insert (f A)) (insert (f B))
             (le A B) (le B A)
             (guard A B) (guard (f A) (f B))")
        .unwrap();
    }
    #[test]
    fn rejects_bad_input() {
        assert!(run("(insert (@lam x x))").is_err());
        assert!(run("(rewrite ?x ?y)").is_err());
        assert!(run("(insert (f x]").is_err());
        assert!(run("(insert a) (fail (guard a b))").is_ok());
        assert!(run("(insert a) (fail (guard a a))").is_err());
        assert!(run("(rewrite-le (f ?x) ?y)").is_err());
        assert!(run("(rewrite-ge (f ?x) ?y)").is_err());
        assert!(run("(refine a b)").is_err());
        assert!(run("(refinement-closure)").is_err());
        assert!(run("(run 2 :unknown)").is_err());
        assert!(run("(run 2 :expand-le :unknown)").is_err());
        assert!(run("(rule ((Rel (f ?x))) (<= ?x ?y))").is_err());
        assert!(run("(rule ((Rel a)) (Rel b))").is_err());
        assert!(run("(fun f (+)) (fun f (-))").is_err());
    }
}
