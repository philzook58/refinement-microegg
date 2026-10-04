# Refinement MicroEgg

A prototype of a refinement egraph based on Max Willsey's microegg <https://github.com/mwillsey/microegg>

Previous discussions of mine:

- <https://www.philipzucker.com/asymmetric_complete/> An Inequality Union Find Inspired by Atomic Asymmetric Completion
- <https://www.philipzucker.com/le_find/> Inequality Union Finds: Baby Steps to Refinement E-graphs

Refinement e-graphs give you an uninterpreted `<=` that is about as baked in as `=` is.

The union find tracks `<=` in a manner similar to an analysis. Tentatively, storing this maximally sparsely rather than fully materializing (DFSing it on demand) as one would in egglog is more performant in memory and time.

Nothing involving `<=` is quite as well behaved or as performant as `=`, but it is there as a light sprinkling on top. If _everything_ you do is refinement rather than equality, I am not sure the refinement egraph offers much over a hash cons with a stored inequality table.

E-matching, rebuilding, and extraction all have slight tweaks related to `<=`. Rebuilding performs refinement closure.

Function symbols can be given a variance signature, very similarly to variance of type parameters in subtyping. This is about whether they are monotone `a <= b -> f a <= f b`, anti-monotone `a <= b -> f a >= f b` or neither in particular arguments. This changes patterns and extraction "modes" appropriately as they go through the term.

Refinement rebuilding / closure is no where near as nice as equality (although it is still conceptually simple). It is not obvious that it will terminate if one allows new enode creation, so in that sense it is in the same naughty category as rewrite rules.

This is useful perhaps because as the story goes, many rewrites in compilers are not equalities, they are refinements.
