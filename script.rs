//! The non-binder subset of lambda-microegg's command language.
use crate::util::{Sexp, Symbol};
use crate::{EGraph, Rewrite};

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

pub fn run(input: &str) -> Result<Vec<String>, String> {
    let forms = parse(input)?;
    run_forms(&forms)
}

fn run_forms(forms: &[Sexp]) -> Result<Vec<String>, String> {
    let mut eg = EGraph::default();
    let mut rewrites: Vec<Rewrite> = Vec::new();
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
                }
                "insert" => {
                    arity(1)?;
                    term(&args[0], false)?;
                    eg.instantiate(&args[0], &Default::default());
                }
                "union" | "guard" => {
                    arity(2)?;
                    term(&args[0], false)?;
                    term(&args[1], false)?;
                    let a = eg.instantiate(&args[0], &Default::default());
                    let b = eg.instantiate(&args[1], &Default::default());
                    if name == "union" {
                        eg.union(a, b);
                        eg.rebuild();
                    } else if !eg.equivalent(a, b) {
                        return Err(format!("guard failed: {} != {}", args[0], args[1]));
                    }
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
                "run" => {
                    arity(1)?;
                    let limit: usize = atom(&args[0])?
                        .parse()
                        .map_err(|_| "run expects a nonnegative integer".to_string())?;
                    for _ in 0..limit {
                        if !eg.rewrite_step(&rewrites) {
                            break;
                        }
                    }
                }
                "match" => {
                    arity(1)?;
                    term(&args[0], true)?;
                    eg.rebuild();
                    output.push(format!("{} matches", eg.match_count(&args[0])));
                }
                "extract" => {
                    arity(1)?;
                    term(&args[0], false)?;
                    let id = eg.instantiate(&args[0], &Default::default());
                    eg.rebuild();
                    output.push(eg.extract(id).ok_or("no finite term in class")?.to_string());
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
    fn rejects_bad_input() {
        assert!(run("(insert (@lam x x))").is_err());
        assert!(run("(rewrite ?x ?y)").is_err());
        assert!(run("(insert (f x]").is_err());
        assert!(run("(insert a) (fail (guard a b))").is_ok());
        assert!(run("(insert a) (fail (guard a a))").is_err());
    }
}
