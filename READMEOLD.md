# Refinement MicroEgg

Max Willsey's simple `microegg` e-graph, wrapped in a Clap CLI for the ordinary
S-expression commands from [lambda-microegg](https://github.com/philzook58/lambda-microegg).

```sh
cargo run --release -- examples/ac10.sexp
cat examples/ac10.sexp | cargo run --release -- -
```

## Browser demo

The [browser page](index.html) runs the same script interpreter through WebAssembly
and offers five editable examples. Build and smoke-test it locally with
`./build-web.sh`, then serve the repository directory with
`python3 -m http.server 8000` and open `http://localhost:8000/`.
The GitHub Pages workflow builds and deploys the page on pushes to `main` or
`master`; the repository's Pages source should be set to GitHub Actions.

The CLI accepts `insert`, `union`, `rewrite`, `birewrite`, `rewrite-le`, `rewrite-ge`, `rule`, `fun`,
`le`, `guard-le`, `run`, `refinement-closure`, `match`, `guard`, `extract`, `extract-le`,
`extract-ge`, `echo`, `fail`, `reset`, and `print-egraph`. Terms use `(f a b)`;
`[f x]` is encoded as `(app f x)`. Semicolon comments and quoted strings are
accepted. `run N` performs at most N rewrite rounds; `run N :expand-le` also
creates variance variants during those rounds. `extract` chooses a term
with the fewest nodes. Binder forms, explicit substitution, outer context
variables, and Miller metavariable occurrences are outside this project's scope.

`(rewrite-le LEFT RIGHT)` means `LEFT <= RIGHT`; `(rewrite-ge LEFT RIGHT)`
means `LEFT >= RIGHT`. The former can match a class `C <= LEFT` and record
`C <= RIGHT`; the latter can match `C >= LEFT` and record `C >= RIGHT`.
Matching searches reachable e-classes and follows declared variance through
the pattern. Covariant arguments keep the search direction, contravariant
arguments reverse it, and invariant arguments match exactly. Equality rewrites
still match exact e-classes. Variables on the right must occur on the left.

`(rule (PREMISE ...) CONCLUSION)` joins premises using one substitution.
Premises may be `(= A B)`, `(<= A B)`, `(>= A B)`, or `(Rel TERM)`; a bare
term is shorthand for `(Rel TERM)`. `Rel` checks that the term pattern occurs
in the e-graph. Comparisons match their term patterns in existing e-classes
and check equality or the known order between those classes. A comparison
already checks that both of its terms exist, so it needs no separate `Rel`
premise for either term. Conclusions
assert `=`, `<=`, or `>=`; their variables must be bound by premises. For
example, [relation_algebra.sexp](examples/relation_algebra.sexp) derives the
residual law `J ∘ X <= G` implies `X <= G / J`.

`(le A B)` asserts a ground fact; `(guard-le A B)` checks whether it follows
from known facts and transitivity. Mutual `<=` becomes equality. `(fun f (+ - =))`
declares covariant, contravariant, and equality-only arguments. Ordinary `run`
rounds derive order edges between existing enodes without creating variance
variants. `(run N :expand-le)` creates variants and their order edges while
applying rules; a rule can match a newly created variant in a later round.
The option applies only to that run, and `N` bounds growth on cyclic e-graphs.
`(refinement-closure N)` remains available for variance-only passes.
`extract-le` chooses the smallest term from classes known to be `<=` its
argument; `extract-ge` does the same for classes known to be `>=` it. Both
include the argument's equality class. Run the set-inclusion example with
`cargo run --release -- examples/refinement.sexp`.
This order does not enforce that a circuit has a nonempty output set at every
input; that would need a separate validity condition.

The circuit don't-care example in `examples/dontcare.sexp` uses
`run 5 :expand-le` with
`(fun ite (= + +))`. With `true <= dontcare` and `false <= dontcare`, it derives
`x <= (ite x true dontcare)` and extracts `x` as a refinement:

```sh
cargo run --release -- examples/dontcare.sexp
# x
```

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

## AC with refinement timing

`examples/ac_le.sexp` checks that covariance and AC together prove an inclusion
between differently ordered union terms. Run it with
`cargo run --release -- examples/ac_le.sexp`.

For a size sweep, run `profile_ac_le` with a leaf count. Each invocation reports
AC alone, order propagation alone, and both together. The order cases use a
chain `x0 <= x1 <= ... <= xn`; every target term is inserted before saturation.
The CSV reports setup and saturation times separately, along with rounds and
graph size. At six leaves, AC alone saturates in 7 rounds, order-only in 28,
and AC with order in 17. For order-only, every labeling of each left-associated
prefix is materialized: with `n` leaves and `n+1` available atoms, the number
of classes is exactly `(n+1) + sum((n+1)^k for k=2..n)`. This is 9,330 at five
leaves and 137,256 at six. The default round limit is 30.

```sh
cargo build --release --example profile_ac_le
for n in 3 4 5; do target/release/examples/profile_ac_le "$n"; done
```

Set `PROFILE_ROUNDS=1` when running the example to print each round's time and
class/node counts to standard error.

The third argument selects the experimental order index: `sparse` (default)
keeps direct edges and searches on demand; `closure` materializes all known
`<=` pairs in bitsets during rebuild. Until rebuild, queries use the sparse
graph. The optional fourth argument sets the number of `is_le` queries after
saturation (default 1,000); `query_ms` reports their time separately. The
optional fifth argument selects one workload. A sixth argument selects
variance propagation: `eager` (default) creates neighboring enodes,
`cartesian` enumerates argument cones and looks up only existing enodes, and
`pairwise` compares existing enodes with the same symbol. The CSV also reports
time spent in variance propagation, candidate checks, and order edges added.
For example:

```sh
target/release/examples/profile_ac_le 4 20 closure 1000
target/release/examples/profile_ac_le 6 30 closure 1000 combined
target/release/examples/profile_ac_le 6 30 sparse 0 combined cartesian
```

With `profile_ac_le`'s default eager variance loop and rebuilds only after changes, at six leaves
sparse versus closure saturation took about 4.5 versus 39 seconds for
order-only, and 3.2 versus 3.9 seconds for AC with order. Peak memory was
about 89 MB versus 4.7 GB for order-only, and 61 MB versus 1.0 GB for AC
with order. Closure still makes point queries much faster after
saturation. At seven leaves, the order-only closure would require about 1.3 TiB
just for its two bitset matrices, so the example caps closure runs at six
leaves. This benchmark has no ordered rewrite rules, so its matcher does not
perform directional matching; it is mostly a saturation workload.

The experimental existing-enode strategies give a different cost profile
with the sparse order index (release build, one local run; RSS measured on the
standalone executable):

| Workload | Variance | Saturation | Classes | Enodes | Peak RSS |
| --- | --- | ---: | ---: | ---: | ---: |
| AC6 order-only | eager | 5.0 s | 137,256 | 137,256 | 89 MB |
| AC6 order-only | cartesian | <0.1 ms | 17 | 17 | 2.5 MB |
| AC6 order-only | pairwise | <0.1 ms | 17 | 17 | 2.6 MB |
| AC6 with order | eager | 2.9 s | 1,715 | 35,336 | 61 MB |
| AC6 with order | cartesian | 24 ms | 95 | 1,031 | 3.3 MB |
| AC6 with order | pairwise | 1.7 s | 95 | 1,031 | 3.7 MB |
| AC7 with order | cartesian | 120 ms | 191 | 3,270 | 5.9 MB |
| AC7 with order | pairwise | 44 s | 191 | 3,270 | 6.0 MB |

In AC6 with order, Cartesian tried about 334,000 tuple lookups; pairwise
tested about 4.65 million ordered enode pairs. At AC7 those counts were
2.4 million and 41.9 million, respectively.

All these runs saturated and proved the benchmark goal. A regression test
compares the inferred order on the common AC4 terms. The default Cartesian
strategy does not create variance variants. Directional ordered rewrites can
match through the order, but an exact equality rewrite cannot match a variant
that was never created. For example, `(f a)` and `a <= b` do not create
`(f b)` during an ordinary `run`, so an equality rewrite matching `(f b)`
needs `run N :expand-le`.
