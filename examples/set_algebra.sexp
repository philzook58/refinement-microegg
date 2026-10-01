; Set algebra: union, intersection, and relative difference.
; union A B = A ∪ B, inter A B = A ∩ B, diff A B = A \ B.
; empty = ∅, univ = the chosen universe. Operators are binary.
;
; Sources:
; https://en.wikipedia.org/wiki/Algebra_of_sets
; https://en.wikipedia.org/wiki/List_of_set_identities_and_relations

; Union and intersection: commutativity, associativity, identity,
; idempotence, and absorption.
(rewrite (union ?a ?b) (union ?b ?a))
(rewrite (inter ?a ?b) (inter ?b ?a))
(rewrite (union (union ?a ?b) ?c) (union ?a (union ?b ?c)))
(rewrite (inter (inter ?a ?b) ?c) (inter ?a (inter ?b ?c)))
(rewrite (union ?a empty) ?a)
(rewrite (inter ?a univ) ?a)
(rewrite (union ?a ?a) ?a)
(rewrite (inter ?a ?a) ?a)
(rewrite (union ?a univ) univ)
(rewrite (inter ?a empty) empty)
(rewrite (union ?a (inter ?a ?b)) ?a)
(rewrite (inter ?a (union ?a ?b)) ?a)
; Each operation also distributes over the other.
(rewrite (inter ?a (union ?b ?c)) (union (inter ?a ?b) (inter ?a ?c)))
(rewrite (union ?a (inter ?b ?c)) (inter (union ?a ?b) (union ?a ?c)))

; Relative difference. These are oriented to keep this small example finite.
(rewrite (diff ?a empty) ?a)
(rewrite (diff empty ?a) empty)
(rewrite (diff ?a ?a) empty)
(rewrite (diff ?a univ) empty)
(rewrite (diff (diff ?a ?b) ?c) (diff ?a (union ?b ?c)))
(rewrite (diff ?a (union ?b ?c)) (inter (diff ?a ?b) (diff ?a ?c)))
(rewrite (diff ?a (inter ?b ?c)) (union (diff ?a ?b) (diff ?a ?c)))
(rewrite (diff (union ?a ?b) ?c) (union (diff ?a ?c) (diff ?b ?c)))
(rewrite (diff (inter ?a ?b) ?c) (inter (diff ?a ?c) (diff ?b ?c)))

; Exercise all three operators, then check familiar identities.
(insert (union (inter A B) (union A empty)))
(insert (inter (union A B) (inter A univ)))
(insert (diff (diff A B) C))
(insert (diff A (inter B C)))
(insert (diff (union A B) C))
(insert (diff (inter A B) C))
(insert (inter A (union B C)))
(insert (union A (inter B C)))
(run 5)

(guard (union (inter A B) (union A empty)) A)
(guard (inter (union A B) (inter A univ)) A)
(guard (diff (diff A B) C) (diff A (union B C)))
(guard (diff A (union B C)) (inter (diff A B) (diff A C)))
(guard (diff A (inter B C)) (union (diff A B) (diff A C)))
(guard (diff (union A B) C) (union (diff A C) (diff B C)))
(guard (diff (inter A B) C) (inter (diff A C) (diff B C)))
(guard (inter A (union B C)) (union (inter A B) (inter A C)))
(guard (union A (inter B C)) (inter (union A B) (union A C)))

(extract (union (inter A B) (union A empty)))
