use mcrl2::Pbes;
use mcrl2::SrfPbes;
use merc_explore::CachingStrategy;
use merc_explore::ExplorationStrategy;
use merc_symbolic::ExplorationStrategy as SymbolicExplorationStrategy;
use merc_symbolic::LDD_CACHE_CAPACITY;
use merc_symbolic::LDD_NODE_CAPACITY;
use merc_symbolic::SymbolicLpsOptions;
use merc_utilities::Timing;
use merc_vpg::ExtendedParityGame;
use merc_vpg::PG;
use merc_vpg::ParityGameBuilder;
use merc_vpg::Player;
use merc_vpg::VertexIndex;
use merc_vpg::solve_symbolic_zielonka;
use merc_vpg::solve_zielonka;
use merc_vpg::verify_symbolic_strategy;

use merc_pbes::explore_pbes_symbolic_game;
use merc_pbes::explore_srf_pbes;

/// Converts `pbes` to SRF form and unifies its parameter lists for the explicit path.
fn unified_srf(pbes: &Pbes) -> SrfPbes {
    let mut srf = SrfPbes::from(pbes).expect("Failed to convert to SRF");
    srf.unify_parameters(false, true).expect("Failed to unify parameters");
    srf
}

/// Converts `pbes` to SRF form and unifies its parameter lists for the symbolic path.
fn unified_symbolic_srf(pbes: &Pbes) -> SrfPbes {
    let mut srf = SrfPbes::from(pbes).expect("Failed to convert to SRF");
    srf.unify_parameters(true, false).expect("Failed to unify parameters");
    srf
}

/// Solves `pbes` both explicitly (SRF exploration + Zielonka) and symbolically (LDD reachability +
/// the symbolic Zielonka solver), and asserts the initial vertex has the same winner in both.
fn assert_symbolic_matches_explicit(pbes: &Pbes) {
    // Normalise to positive normal form so the SRF converter accepts it.
    let mut pbes = pbes.clone();
    pbes.normalize();
    let pbes = &pbes;

    let game = explore_srf_pbes(
        unified_srf(pbes),
        ExplorationStrategy::Bfs,
        CachingStrategy::None,
        false,
        &Timing::new(),
        ParityGameBuilder::new(VertexIndex::new(0)),
    )
    .expect("SRF exploration failed");
    let (solution, _) = solve_zielonka(&game, false);
    let explicit_winner = if solution[Player::Even.to_index()][game.initial_vertex().value()] {
        Player::Even
    } else {
        Player::Odd
    };

    let storage = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
    let symbolic = explore_pbes_symbolic_game(
        &storage,
        unified_symbolic_srf(pbes),
        &SymbolicLpsOptions::default(),
        SymbolicExplorationStrategy::default(),
        false,
        true,
        &Timing::new(),
    )
    .expect("symbolic exploration failed");

    let epg = ExtendedParityGame::new(symbolic.game, symbolic.initial_vertex, symbolic.sinks);

    // Force a full solve (no early termination).
    let (symbolic_winner, symbolic_solution) = solve_symbolic_zielonka(&epg, false).expect("symbolic solve failed");

    assert_eq!(
        explicit_winner, symbolic_winner,
        "explicit and symbolic solvers disagree on the initial vertex's winner"
    );

    verify_symbolic_strategy(&epg.game, &epg.initial_vertex, &symbolic_solution)
        .expect("native strategy certification failed");
}

fn assert_symbolic_matches_explicit_from_text(text: &str) {
    let pbes = Pbes::from_text(text).expect("Failed to parse PBES");
    assert_symbolic_matches_explicit(&pbes);
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_simple_mu_true() {
    assert_symbolic_matches_explicit_from_text("pbes mu X = val(true); init X;");
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_simple_mu_false() {
    assert_symbolic_matches_explicit_from_text("pbes mu X = val(false); init X;");
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_simple_nu_true() {
    assert_symbolic_matches_explicit_from_text("pbes nu X = val(true); init X;");
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_simple_nu_false() {
    assert_symbolic_matches_explicit_from_text("pbes nu X = val(false); init X;");
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_symbolic_a_text_pbes() {
    assert_symbolic_matches_explicit_from_text(include_str!("../../../../../examples/pbes/a.text.pbes"));
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_symbolic_b_text_pbes() {
    assert_symbolic_matches_explicit_from_text(include_str!("../../../../../examples/pbes/b.text.pbes"));
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_symbolic_c_text_pbes() {
    assert_symbolic_matches_explicit_from_text(include_str!("../../../../../examples/pbes/c.text.pbes"));
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_symbolic_random_pbes_seeds() {
    use merc_syntax::random_pbes;
    use rand::SeedableRng;

    for seed in 0u64..50 {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);

        // Propositional only (no quantifiers, no integers): the symbolic path enumerates every
        // parameter's domain when interning it into an LDD value, so it needs a finite one.
        let pbes_ast = random_pbes(&mut rng, 0, 0, 3, 2, 3, false, false);
        let text = pbes_ast.to_string();
        assert_symbolic_matches_explicit_from_text(&text);
    }
}
