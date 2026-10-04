; Relational division: J ∘ X <= G implies X <= G / J.
; Here comp J X denotes J ∘ X, and rdiv G J denotes G / J.
; The inequality premise itself matches the existing composition.
(insert (comp J X))
(le (comp J X) G)
(rule ((<= (comp ?j ?x) ?g))
      (<= ?x (rdiv ?g ?j)))
(run 1)
(guard-le X (rdiv G J))
(echo "division passed")
