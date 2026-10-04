; A Boolean circuit returns a set of possible outputs at each input.
; true and false denote singleton outputs; dontcare denotes {true, false}.
; Inclusion is the order: a deterministic implementation <= its specification.
(fun ite (= + +))
(rewrite (ite ?x true false) ?x)
(le true dontcare)
(le false dontcare)

(insert (ite x true dontcare))
(run 5 :expand-le)
(guard-le (ite x true false) (ite x true dontcare))
(guard (ite x true false) x)
(guard-le x (ite x true dontcare))
(fail (guard-le (ite x true dontcare) x))
(extract-le (ite x true dontcare))
