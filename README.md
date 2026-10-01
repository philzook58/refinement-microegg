# Refinement MicroEgg

Max Willsey's simple `microegg` e-graph, wrapped in a Clap CLI for the ordinary
S-expression commands from [lambda-microegg](https://github.com/philzook58/lambda-microegg).

```sh
cargo run --release -- examples/ac10.sexp
cat examples/ac10.sexp | cargo run --release -- -
```

The CLI accepts `insert`, `union`, `rewrite`, `birewrite`, `run`, `match`,
`guard`, `extract`, `echo`, `fail`, `reset`, and `print-egraph`. Terms use `(f a b)`;
`[f x]` is encoded as `(app f x)`. Semicolon comments and quoted strings are
accepted. `run N` performs at most N rewrite rounds. `extract` chooses a term
with the fewest nodes. Binder forms, explicit substitution, outer context
variables, and Miller metavariable occurrences are outside this project's scope.

```sh
printf '%s\n' '(insert (+ a 0))' '(rewrite (+ ?x 0) ?x)' '(run 3)' '(extract (+ a 0))' |
  cargo run --release -- -
# a
```

For set union, intersection, and difference identities, run
`cargo run --release -- examples/set_algebra.sexp`. The example checks the
identities with `guard` and extracts a simplified result.

## AC10 timing

`examples/ac10.sexp` inserts a left-associated term over ten distinct atoms,
saturates with associativity and commutativity, then checks its reversed form.
For the original library test, use:

```sh
env n=10 cargo test --release test_ac_rewriting -- --exact --nocapture
```

The library test checks 1,023 e-classes. Time the prebuilt CLI without
including compilation:

```sh
cargo build --release
/usr/bin/time -f 'elapsed=%e s max_rss=%M KB' \
  target/release/refinement-microegg examples/ac10.sexp
```

To break down the library test's rewrite loop by round, run
`cargo run --release --example profile_ac10`. It reports matching, applying,
and rebuilding times, plus matches, newly inserted nodes, and successful unions.
