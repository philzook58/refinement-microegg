; Directional refinement uses set inclusion: LEFT <= RIGHT.
; No variance declarations or inequality-aware matching are needed here.
(insert (inter A B))
(insert (diff A B))
(rewrite-le (inter ?a ?b) ?a)
(rewrite-le (inter ?a ?b) ?b)
(rewrite-le (diff ?a ?b) ?a)
(le A Top)
(run 3)

(guard-le (inter A B) A)
(guard-le (inter A B) B)
(guard-le (inter A B) Top)
(guard-le (diff A B) A)
(fail (guard-le A (inter A B)))

; Mutual <= collapses to equality, including parent enodes by congruence.
(reset)
(insert (f A))
(insert (f B))
(le A B)
(le B A)
(guard A B)
(guard (f A) (f B))
(echo "refinement guards passed")
