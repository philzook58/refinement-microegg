; Associativity/commutativity saturation with ten atoms.
(insert (f (f (f (f (f (f (f (f (f x0 x1) x2) x3) x4) x5) x6) x7) x8) x9))
(rewrite (f (f ?x ?y) ?z) (f ?x (f ?y ?z)))
(rewrite (f ?x ?y) (f ?y ?x))
(run 20)
(guard (f (f (f (f (f (f (f (f (f x0 x1) x2) x3) x4) x5) x6) x7) x8) x9)
       (f (f (f (f (f (f (f (f (f x9 x8) x7) x6) x5) x4) x3) x2) x1) x0))
