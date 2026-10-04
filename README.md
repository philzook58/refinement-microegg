# Refinement MicroEgg

A prototype of a refinement egraph based on Max Willsey's microegg <https://github.com/mwillsey/microegg>. A WASM demo is here <https://www.philipzucker.com/refinement-microegg>

Refinement e-graphs give you an uninterpreted `<=` that is about as baked in as `=` is.

This is useful perhaps because as the story goes, many rewrites in compilers are not unoriented equalities, they are oriented refinements.

`<=` is baked in to be transitive, reflexive, and collapses cycles to `=`.

Previous discussions of mine on refinement e-graphs:

- <https://www.philipzucker.com/asymmetric_complete/> An Inequality Union Find Inspired by Atomic Asymmetric Completion
- <https://www.philipzucker.com/le_find/> Inequality Union Finds: Baby Steps to Refinement E-graphs

The union find tracks upper and lower `<=` sets in a manner similar to an analysis (they are keyed on eclass and merge on union). Tentatively, storing this maximally sparsely rather than fully materializing (DFSing it on demand) as one would in egglog is more performant in memory and time. [Egglog](https://github.com/egraphs-good/egglog) itself is highly engineered though, so I don't actually know how this shakes out.

In either case, I think baking in `<=` rather than having it as a mangled program or macro is conceptually cohesive and pleasant.

Nothing involving `<=` is quite as well behaved or as performant as `=`, but it is there as a light sprinkling on top. If _everything_ you do is refinement rather than equality, I am not sure the refinement egraph offers much over a hash cons with a stored inequality table.

E-matching, rebuilding, and extraction all have slight tweaks related to `<=`. Rebuilding performs refinement closure.

Function symbols can be given a variance signature, very similarly to variance of type parameters in subtyping. This is about whether they are monotone `a <= b -> f a <= f b`, anti-monotone `a <= b -> f a >= f b` or neither in particular arguments. This changes patterns and extraction "modes" appropriately as they go through the term.

Refinement rebuilding / closure is no where near as nice as equality (although it is still conceptually simple). It is not obvious that it will terminate if one allows new enode creation, so in that sense it is in the same naughty category as rewrite rules. There is a distinction to be made between materializing and non-materializing refinement closure (only note inequalities between pre-exising enodes).

# Circuit Don't Care Example

A nice example is "don't care" in boolean circuits <https://en.wikipedia.org/wiki/Don%27t-care_term> . Some inputs are not expected or allowed, so they optimizer is free to pick a behavior on those inputs that helps make a more optimal circuit. I believe George Constantinides explained this to me.

The intended semantics of this example is `Bool -> Set Bool`. `[[x]] = fun b => {b}` is the lifted identity function. `ite` is pointwise lifted. `[[dontcare]] = fun _ => {True, False}` `[[true]] = fun _ => {True}` `[[false]] = fun _ => {False}`. Refinement is interpreted as subrelation.

```
(fun ite (+ + +)) ; declare if-then-else as covariant in all arguments
(rewrite (ite ?x true false) ?x)  ; ordinary equality rewrite
(ge dontcare true)    ; dontcare refines to true
(ge dontcare false)   ; dontcare also refines to false

(insert (ite x true dontcare)) ; insert starting term
(run 5 :expand-le)

(extract-le (ite x true dontcare))  ; can extract x because refining this dontcare to true enables a nice term
```

# AI Disclosure

This was produced by giving an agent my notes, Max's microegg, and directions.

# Citing

```
@software{refinementmicroegg2026,
  author = {Philip Zucker},
  title = {{Refinement MicroEgg}},
  url = {https://github.com/philzook58/refinement-microegg},
  month = {10},
  year = {2026}
}
```