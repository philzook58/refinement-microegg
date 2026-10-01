use refinement_microegg::{EGraph, util::sexp};
use std::time::{Duration, Instant};

fn main() {
    let n = std::env::args()
        .nth(1)
        .map_or(10, |value| value.parse().unwrap());
    let mut eg = EGraph::default();
    let atoms: Vec<_> = (0..n).map(|i| format!("x{i}")).collect();
    let fold = |items: &[String]| {
        items
            .iter()
            .cloned()
            .reduce(|acc, item| format!("(f {acc} {item})"))
            .unwrap()
    };
    let input = eg.add(&fold(&atoms));
    let goal = eg.add(&fold(&atoms.into_iter().rev().collect::<Vec<_>>()));
    let rewrites = [
        (sexp("(f (f ?x ?y) ?z)"), sexp("(f ?x (f ?y ?z))")),
        (sexp("(f ?x ?y)"), sexp("(f ?y ?x)")),
    ];
    let start = Instant::now();
    eg.rebuild();
    let initial_rebuild = start.elapsed();
    let (mut matching, mut applying, mut rebuilding) =
        (Duration::ZERO, Duration::ZERO, initial_rebuild);
    println!("round,matches,new_nodes,unions,classes,nodes,match_ms,apply_ms,rebuild_ms");
    for round in 1.. {
        let step = eg.profile_step(&rewrites);
        matching += step.matching;
        applying += step.applying;
        rebuilding += step.rebuilding;
        let (classes, nodes) = eg.statistics();
        println!(
            "{round},{},{},{},{classes},{nodes},{:.3},{:.3},{:.3}",
            step.matches,
            step.new_nodes,
            step.unions,
            step.matching.as_secs_f64() * 1e3,
            step.applying.as_secs_f64() * 1e3,
            step.rebuilding.as_secs_f64() * 1e3
        );
        if !step.changed {
            break;
        }
    }
    assert!(eg.equivalent(input, goal));
    eprintln!(
        "total: match={:.3}s apply={:.3}s rebuild={:.3}s initial_rebuild={:.3}s",
        matching.as_secs_f64(),
        applying.as_secs_f64(),
        rebuilding.as_secs_f64(),
        initial_rebuild.as_secs_f64()
    );
}
