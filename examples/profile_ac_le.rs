//! Compare AC saturation, covariant order propagation, and their combination.
use refinement_microegg::{EGraph, Variance, VarianceStrategy, util::sexp};
use std::time::Instant;

#[derive(Clone, Copy)]
enum Mode {
    Ac,
    Order,
    Combined,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Ac => "ac",
            Self::Order => "order",
            Self::Combined => "combined",
        }
    }

    fn has_ac(self) -> bool {
        matches!(self, Self::Ac | Self::Combined)
    }

    fn has_order(self) -> bool {
        matches!(self, Self::Order | Self::Combined)
    }
}

fn fold(items: &[String]) -> String {
    items
        .iter()
        .cloned()
        .reduce(|left, right| format!("(U {left} {right})"))
        .unwrap()
}

fn measure(
    mode: Mode,
    n: usize,
    max_rounds: usize,
    materialized: bool,
    queries: usize,
    strategy: VarianceStrategy,
) {
    let setup_start = Instant::now();
    let mut eg = if materialized {
        EGraph::with_materialized_order()
    } else {
        EGraph::default()
    };
    eg.set_variance_strategy(strategy);
    let atoms: Vec<_> = (0..=n).map(|i| format!("x{i}")).collect();
    let input = eg.add(&fold(&atoms[..n]));

    if mode.has_order() {
        eg.declare_variance("U".into(), vec![Variance::Covariant; 2])
            .unwrap();
        for pair in atoms.windows(2) {
            let lower = eg.add(&pair[0]);
            let upper = eg.add(&pair[1]);
            eg.assert_le(lower, upper);
        }
    }

    let mut target_atoms = if mode.has_order() {
        atoms[1..].to_vec()
    } else {
        atoms[..n].to_vec()
    };
    if mode.has_ac() {
        target_atoms.reverse();
    }
    let goal = eg.add(&fold(&target_atoms));
    let rewrites = if mode.has_ac() {
        vec![
            (sexp("(U (U ?x ?y) ?z)"), sexp("(U ?x (U ?y ?z))")),
            (sexp("(U ?x ?y)"), sexp("(U ?y ?x)")),
        ]
    } else {
        vec![]
    };
    eg.rebuild();
    let setup_ms = setup_start.elapsed().as_secs_f64() * 1e3;

    let run_start = Instant::now();
    let mut rounds = 0;
    let mut saturated = false;
    let trace_rounds = std::env::var_os("PROFILE_ROUNDS").is_some();
    for _ in 0..max_rounds {
        rounds += 1;
        let round_start = Instant::now();
        let changed = eg.refinement_step(&rewrites, &[]);
        if trace_rounds {
            let (classes, nodes) = eg.statistics();
            eprintln!(
                "{},round={rounds},changed={changed},classes={classes},nodes={nodes},ms={:.3}",
                mode.name(),
                round_start.elapsed().as_secs_f64() * 1e3
            );
        }
        if !changed {
            saturated = true;
            break;
        }
    }
    let run_ms = run_start.elapsed().as_secs_f64() * 1e3;
    let proved = if mode.has_order() {
        eg.is_le(input, goal)
    } else {
        eg.equivalent(input, goal)
    };
    let (classes, nodes) = eg.statistics();
    let mut candidates = eg.upper_classes(input);
    candidates.sort_unstable();
    let mut seed = 1u32;
    let mut true_queries = 0;
    let query_start = Instant::now();
    for _ in 0..queries {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let a = candidates[(seed as usize) % candidates.len()];
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let b = candidates[(seed as usize) % candidates.len()];
        true_queries += usize::from(eg.is_le(a, b));
    }
    let query_ms = query_start.elapsed().as_secs_f64() * 1e3;
    let variance = eg.variance_profile();
    println!(
        "{},{},{},{n},{rounds},{saturated},{proved},{classes},{nodes},{setup_ms:.3},{run_ms:.3},{:.3},{},{},{queries},{true_queries},{query_ms:.3}",
        if materialized { "closure" } else { "sparse" },
        mode.name(),
        match strategy {
            VarianceStrategy::Eager => "eager",
            VarianceStrategy::Cartesian => "cartesian",
            VarianceStrategy::Pairwise => "pairwise",
        },
        variance.time.as_secs_f64() * 1e3,
        variance.candidates,
        variance.edges_added,
    );
    assert!(
        proved,
        "{} goal not proved within {max_rounds} rounds",
        mode.name()
    );
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n: usize = args.next().map_or(4, |value| value.parse().unwrap());
    let max_rounds: usize = args.next().map_or(30, |value| value.parse().unwrap());
    let index = args.next().unwrap_or_else(|| "sparse".into());
    let queries: usize = args.next().map_or(1000, |value| value.parse().unwrap());
    let selected_mode = args.next().unwrap_or_else(|| "all".into());
    let strategy = match args.next().as_deref().unwrap_or("eager") {
        "eager" => VarianceStrategy::Eager,
        "cartesian" => VarianceStrategy::Cartesian,
        "pairwise" => VarianceStrategy::Pairwise,
        other => panic!("unknown variance strategy {other}"),
    };
    assert!(n >= 2, "need at least two leaves");
    assert!(max_rounds > 0, "need at least one round");
    assert!(
        index == "sparse" || index == "closure",
        "index must be sparse or closure"
    );
    assert!(
        ["all", "ac", "order", "combined"].contains(&selected_mode.as_str()),
        "mode must be all, ac, order, or combined"
    );
    assert!(
        index != "closure" || n <= 5 || (n == 6 && selected_mode != "all"),
        "closure at six leaves requires selecting one mode"
    );
    assert!(
        args.next().is_none(),
        "usage: profile_ac_le [leaves] [max_rounds] [sparse|closure] [queries] [all|ac|order|combined] [eager|cartesian|pairwise]"
    );

    println!(
        "index,mode,variance_strategy,leaves,rounds,saturated,proved,classes,nodes,setup_ms,run_ms,variance_ms,candidates,edges_added,queries,true_queries,query_ms"
    );
    for mode in [Mode::Ac, Mode::Order, Mode::Combined] {
        if selected_mode == "all" || selected_mode == mode.name() {
            measure(mode, n, max_rounds, index == "closure", queries, strategy);
        }
    }
}
