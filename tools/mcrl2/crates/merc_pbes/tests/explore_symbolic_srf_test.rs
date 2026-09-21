use std::path::Path;

use mcrl2::Pbes;
use mcrl2::SrfPbes;
use merc_symbolic::LDD_CACHE_CAPACITY;
use merc_symbolic::LDD_NODE_CAPACITY;
use merc_explore::CachingStrategy;
use merc_explore::ExplorationStrategy;
use merc_symbolic::SymbolicLpsOptions;
use merc_utilities::Timing;
use merc_vpg::PG;
use merc_vpg::ParityGameBuilder;
use merc_vpg::VertexIndex;

use merc_pbes::explore_pbes_symbolic;
use merc_pbes::explore_srf_pbes;

/// Converts `pbes` to SRF form and unifies its parameter lists, as every
/// SRF-based explorer requires (see `PbesSrfLps::new`).
fn unified_srf(pbes: &Pbes) -> SrfPbes {
    let mut srf = SrfPbes::from(pbes).expect("Failed to convert to SRF");
    srf.unify_parameters(false, true).expect("Failed to unify parameters");
    srf
}

/// Reads a textual PBES, explores it both explicitly (into a parity game)
/// and symbolically (into an LDD), and asserts the number of reachable BES
/// equations agrees. The explicit parity game has exactly one vertex per
/// reachable equation, so its vertex count must equal the symbolic state
/// count.
fn assert_symbolic_matches_explicit(text_pbes_relative_path: &str) {
    let text_pbes_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(text_pbes_relative_path);
    assert!(
        text_pbes_path.exists(),
        "Text PBES file not found: {}",
        text_pbes_path.display()
    );

    let pbes = Pbes::from_text_file(text_pbes_path.to_str().unwrap()).expect("Failed to read text PBES");

    let game = explore_srf_pbes(
        unified_srf(&pbes),
        ExplorationStrategy::Bfs,
        CachingStrategy::None,
        false,
        &Timing::new(),
        ParityGameBuilder::new(VertexIndex::new(0)),
    )
    .expect("Failed to build parity game");

    let storage = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
    let timing = Timing::new();
    let mut symbolic_srf = SrfPbes::from(&pbes).expect("Failed to convert to SRF");
    symbolic_srf
        .unify_parameters(true, false)
        .expect("Failed to unify parameters");
    let states = explore_pbes_symbolic(&storage, symbolic_srf, &SymbolicLpsOptions::default(), false, &timing)
        .expect("Failed to explore PBES symbolically");

    assert_eq!(
        states.len() as usize,
        game.num_of_vertices(),
        "Symbolic state count and explicit vertex count differ for {text_pbes_relative_path}"
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_symbolic_a_text_pbes() {
    assert_symbolic_matches_explicit("../../../../examples/pbes/a.text.pbes");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_symbolic_b_text_pbes() {
    assert_symbolic_matches_explicit("../../../../examples/pbes/b.text.pbes");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_symbolic_c_text_pbes() {
    assert_symbolic_matches_explicit("../../../../examples/pbes/c.text.pbes");
}
