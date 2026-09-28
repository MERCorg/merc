use std::path::Path;
use std::process::Command;

use mcrl2::Pbes;
use mcrl2::SrfPbes;
use merc_explore::CachingStrategy;
use merc_explore::ExplorationStrategy;
use merc_io::temp_dir;
use merc_io::traced_command;
use merc_utilities::Timing;
use merc_vpg::PG;
use merc_vpg::ParityGameBuilder;
use merc_vpg::VertexIndex;
use merc_vpg::solve_zielonka;

use merc_pbes::explore_pbes;
use merc_pbes::explore_pbes_parallel;
use merc_pbes::explore_srf_pbes;

/// Converts `pbes` to SRF form and unifies its parameter lists, as every
/// SRF-based explorer requires (see `PbesSrfLps::new`).
fn unified_srf(pbes: &Pbes) -> SrfPbes {
    let mut srf = SrfPbes::from(pbes).expect("Failed to convert to SRF");
    srf.unify_parameters(false, true).expect("Failed to unify parameters");
    srf
}

/// Explores `pbes` with both the SRF path and the general path, solves each
/// parity game with Zielonka, and asserts the initial-vertex winner agrees.
fn assert_general_matches_srf(pbes: &Pbes) {
    // Normalise to positive normal form so the SRF converter accepts it.
    let mut normalised = pbes.clone();
    normalised.normalize();

    let game_srf = explore_srf_pbes(
        unified_srf(&normalised),
        ExplorationStrategy::Bfs,
        CachingStrategy::None,
        false,
        &Timing::new(),
        ParityGameBuilder::new(VertexIndex::new(0)),
    )
    .expect("SRF exploration failed");
    let game_gen = explore_pbes(
        normalised,
        ExplorationStrategy::Bfs,
        &Timing::new(),
        ParityGameBuilder::new(VertexIndex::new(0)),
    )
    .expect("General exploration failed");

    let (sol_srf, _) = solve_zielonka(&game_srf, false);
    let (sol_gen, _) = solve_zielonka(&game_gen, false);

    assert_eq!(
        sol_srf[0][0], sol_gen[0][0],
        "SRF and general explorers disagree on initial-vertex winner"
    );
}

fn assert_general_matches_srf_from_text(text: &str) {
    let pbes = Pbes::from_text(text).expect("Failed to parse PBES");
    assert_general_matches_srf(&pbes);
}

#[test]
fn test_simple_mu_true() {
    // mu X = val(true); solution: false (cannot prove true iteratively from bottom)
    // Actually mu X = val(true) fixpoint: X = true, so solution = true
    assert_general_matches_srf_from_text("pbes mu X = val(true); init X;");
}

#[test]
fn test_simple_mu_false() {
    assert_general_matches_srf_from_text("pbes mu X = val(false); init X;");
}

#[test]
fn test_simple_nu_true() {
    assert_general_matches_srf_from_text("pbes nu X = val(true); init X;");
}

#[test]
fn test_simple_nu_false() {
    assert_general_matches_srf_from_text("pbes nu X = val(false); init X;");
}

#[test]
fn test_mu_self_loop() {
    // mu X = X; init X — least fixpoint of X = X is false
    assert_general_matches_srf_from_text("pbes mu X = X; init X;");
}

#[test]
fn test_nu_self_loop() {
    // nu X = X; init X — greatest fixpoint of X = X is true
    assert_general_matches_srf_from_text("pbes nu X = X; init X;");
}

#[test]
fn test_and_formula() {
    // nu X = X && X — equivalent to nu X = X, solution true
    assert_general_matches_srf_from_text("pbes nu X = X && X; init X;");
}

#[test]
fn test_or_formula() {
    // mu X = X || X — equivalent to mu X = X, solution false
    assert_general_matches_srf_from_text("pbes mu X = X || X; init X;");
}

#[test]
fn test_two_equations_alternation() {
    // nu X = Y; mu Y = X — alternation depth 1
    assert_general_matches_srf_from_text("pbes nu X = Y; mu Y = X; init X;");
}

#[test]
fn test_and_of_two_pvis() {
    // nu X = Y && Z; nu Y = val(true); nu Z = val(true);
    assert_general_matches_srf_from_text("pbes nu X = Y && Z; nu Y = val(true); nu Z = val(true); init X;");
}

#[test]
fn test_or_of_two_pvis() {
    assert_general_matches_srf_from_text("pbes mu X = Y || Z; mu Y = val(false); mu Z = val(false); init X;");
}

#[test]
fn test_nested_and_or() {
    // Nested formula: nu X = (Y && Z) || W
    assert_general_matches_srf_from_text(
        "pbes nu X = (Y && Z) || W; nu Y = val(true); nu Z = val(true); nu W = val(false); init X;",
    );
}

#[test]
fn test_data_param_bool() {
    // PBES with a Bool parameter
    assert_general_matches_srf_from_text("pbes nu X(b: Bool) = val(b); init X(true);");
}

#[test]
fn test_data_param_nat() {
    assert_general_matches_srf_from_text("pbes mu X(n: Nat) = val(n == 0); init X(1);");
}

#[test]
fn test_data_param_with_pvi() {
    assert_general_matches_srf_from_text("pbes nu X(n: Int) = val(n > 0) || X(n - 1); init X(3);");
}

#[test]
fn test_multiple_equations_data() {
    assert_general_matches_srf_from_text(
        "pbes mu X(n: Int) = val(n == 0) || Y(n); \
            nu Y(n: Int) = val(n > 0) && X(n - 1); \
            init X(2);",
    );
}

/// Explores `pbes` with local caching and asserts the result matches the uncached exploration.
///
/// [`explore_pbes`] itself does not cache, since none of [`PbesLps`]'s
/// summands can benefit from it. Its state effects are still consumed whenever a
/// [`CacheLPS`] is composed with it — which is what [`crate::quotient_lps`] does —
/// so the declarations still have to be honest, and this builds that composition
/// explicitly to keep checking them.
fn assert_cached_matches_uncached(pbes: &Pbes) {
    let mut normalised = pbes.clone();
    normalised.normalize();

    let game_uncached = explore_pbes(
        normalised.clone(),
        ExplorationStrategy::Bfs,
        &Timing::new(),
        ParityGameBuilder::new(VertexIndex::new(0)),
    )
    .expect("uncached exploration failed");
    let game_cached = explore_pbes(
        normalised,
        ExplorationStrategy::Bfs,
        &Timing::new(),
        ParityGameBuilder::new(VertexIndex::new(0)),
    )
    .expect("cached exploration failed");

    // The games must be *identical* in size, not merely have the same winner.
    // A summand that mis-declares its state effect mints spurious vertices
    // whose winner often still agrees, which hides the defect.
    assert_eq!(
        game_uncached.num_of_vertices(),
        game_cached.num_of_vertices(),
        "caching changed the number of vertices"
    );
    assert_eq!(
        game_uncached.num_of_edges(),
        game_cached.num_of_edges(),
        "caching changed the number of edges"
    );

    let (sol_uncached, _) = solve_zielonka(&game_uncached, false);
    let (sol_cached, _) = solve_zielonka(&game_cached, false);

    // Both runs explore in the same order from the same initial state, so
    // vertex indices line up and the winning sets must agree everywhere, not
    // just at the initial vertex.
    assert_eq!(
        sol_uncached, sol_cached,
        "cached and uncached explorers disagree on the winner of some vertex"
    );
}

fn assert_cached_matches_uncached_from_text(text: &str) {
    let pbes = Pbes::from_text(text).expect("Failed to parse PBES");
    assert_cached_matches_uncached(&pbes);
}

#[test]
fn test_cached_and_formula() {
    let pbes =
        Pbes::from_text("pbes nu X = Y && Z; nu Y = val(true); nu Z = val(true); init X;").expect("parse failed");
    assert_cached_matches_uncached(&pbes);
}

#[test]
fn test_cached_or_formula() {
    let pbes =
        Pbes::from_text("pbes mu X = Y || Z; mu Y = val(false); mu Z = val(false); init X;").expect("parse failed");
    assert_cached_matches_uncached(&pbes);
}

/// An equation whose right-hand side rewrites to `val(true)` emits a sink,
/// which is shorter than its source state. Replaying that from the cache by
/// scattering write positions onto the source produces a padded, bogus sink.
#[test]
#[cfg_attr(miri, ignore)]
fn test_cached_equation_collapsing_to_a_sink() {
    assert_cached_matches_uncached_from_text(
        "pbes
nu X(n: Nat) = val(n == 3) || (Y(n) && X(n + 1));
nu Y(n: Nat) = val(true);
init X(0);",
    );
}

/// A quantifier carries no syntactic `&&`/`||`, but the rewriter expands it
/// into one, so the equation can emit a subformula vertex whose length
/// differs from the source state.
#[test]
#[cfg_attr(miri, ignore)]
fn test_cached_quantifier_expands_to_subformula() {
    assert_cached_matches_uncached_from_text(
        "pbes
nu X(n: Nat) = forall m: Nat . val(m > 2) || Y(n);
nu Y(n: Nat) = val(n == 0) || X(n + 1);
init X(0);",
    );
}

/// An equation that passes every parameter through unchanged still depends on
/// those parameters: under an opaque effect the whole next state is cached, so
/// a passed-through value has to be part of the cache key.
#[test]
#[cfg_attr(miri, ignore)]
fn test_cached_identity_arguments_are_part_of_the_key() {
    assert_cached_matches_uncached_from_text(
        "pbes
nu X(m: Nat, n: Nat) = Y(m, n) || val(m == n);
nu Y(m: Nat, n: Nat) = val(m == 2) || X(m + 1, n);
init X(0, 1);",
    );
}

/// Explores `pbes` with the parallel explorer under every caching strategy
/// and asserts each result matches the sequential uncached exploration.
///
/// The parallel explorer numbers vertices sparsely (see `explore_parallel`),
/// so its game has extra unreachable deadlock vertices; vertex 0 is the
/// initial state in both, so the comparison is on the solution rather than
/// on the vertex and edge counts.
fn assert_parallel_matches_sequential(pbes: &Pbes) {
    let mut normalised = pbes.clone();
    normalised.normalize();

    let sequential = explore_pbes(
        normalised.clone(),
        ExplorationStrategy::Bfs,
        &Timing::new(),
        ParityGameBuilder::new(VertexIndex::new(0)),
    )
    .expect("sequential exploration failed");
    let (sol_sequential, _) = solve_zielonka(&sequential, false);

    for caching in [CachingStrategy::None, CachingStrategy::Local] {
        let parallel = explore_pbes_parallel(
            normalised.clone(),
            4,
            false,
            &Timing::new(),
            ParityGameBuilder::new(VertexIndex::new(0)),
        )
        .expect("parallel exploration failed");
        let (sol_parallel, _) = solve_zielonka(&parallel, false);

        assert_eq!(
            sol_sequential[0][0], sol_parallel[0][0],
            "parallel explorer with {caching:?} caching disagrees with the sequential one"
        );
    }
}

#[test]
fn test_parallel_and_formula() {
    let pbes =
        Pbes::from_text("pbes nu X = Y && Z; nu Y = val(true); nu Z = val(true); init X;").expect("parse failed");
    assert_parallel_matches_sequential(&pbes);
}

#[test]
fn test_parallel_or_formula() {
    let pbes =
        Pbes::from_text("pbes mu X = Y || Z; mu Y = val(false); mu Z = val(false); init X;").expect("parse failed");
    assert_parallel_matches_sequential(&pbes);
}

#[test]
fn test_parallel_data_param_with_pvi() {
    let pbes = Pbes::from_text("pbes nu X(n: Int) = val(n > 0) || X(n - 1); init X(3);").expect("parse failed");
    assert_parallel_matches_sequential(&pbes);
}

/// The initial state is rewritten before it is explored from, as mCRL2's
/// pbesinst does.
///
/// Every other state is reached through the rewriter, so an initial argument
/// left as written is a value no successor ever equals: `X(1 + 1)` would get
/// a vertex of its own that no edge leads back to, and rewriting its body
/// leaves an unevaluated `1 + 1` behind.
#[test]
fn test_initial_state_is_rewritten() {
    let pbes = Pbes::from_text("pbes nu X(n: Nat) = val(n >= 3) || X(n + 1);\ninit X(1 + 1);").expect("parse failed");

    let game = explore_pbes(
        pbes,
        ExplorationStrategy::Bfs,
        &Timing::new(),
        ParityGameBuilder::new(VertexIndex::new(0)),
    )
    .expect("an initial state that needs rewriting must still be explorable");

    // X(2), X(3) and the true sink: X(1 + 1) is X(2), not a state of its own.
    assert_eq!(game.num_of_vertices(), 3);
}

/// Preprocessing instantiates the global variables, which is what makes a
/// PBES with a global variable in its initial state explorable at all.
#[test]
fn test_preprocess_instantiates_global_variables() {
    let text = "glob g: Nat;\npbes nu X(n: Nat) = val(n >= 3) || X(n + 1);\ninit X(g);";

    let unprocessed = Pbes::from_text(text).expect("parse failed");
    assert!(
        explore_pbes(
            unprocessed,
            ExplorationStrategy::Bfs,
            &Timing::new(),
            ParityGameBuilder::new(VertexIndex::new(0)),
        )
        .is_err(),
        "an uninstantiated global variable cannot be rewritten to a value"
    );

    let mut preprocessed = Pbes::from_text(text).expect("parse failed");
    preprocessed.preprocess(&Timing::new()).expect("preprocessing failed");
    explore_pbes(
        preprocessed,
        ExplorationStrategy::Bfs,
        &Timing::new(),
        ParityGameBuilder::new(VertexIndex::new(0)),
    )
    .expect("after preprocessing the global variable has a value");
}

/// A nested formula reachable from two equations of *different* priority is
/// a single vertex, and merging it does not change who wins.
///
/// `X` (priority 2) and `W` (priority 1) both have `Y(n) || Z(n)` as their
/// first conjunct, so both reach the very same disjunction term. Keying the
/// subformula vertex on the enclosing priority as well as on the term would
/// split it into two vertices here, which is what mCRL2's `SG1` does not do
/// either: `insert_vertex(psi)` is keyed on the formula alone.
#[test]
fn test_shared_subformula_is_one_vertex() {
    let pbes = Pbes::from_text(
        "pbes nu X(n: Nat) = (Y(n) || Z(n)) && W(n);
                mu W(n: Nat) = (Y(n) || Z(n)) && X(n);
                nu Y(n: Nat) = val(n > 0);
                nu Z(n: Nat) = val(n < 1);
            init X(0);",
    )
    .expect("parse failed");

    // The SRF explorer has no subformula vertices at all, so it is an
    // independent check that the merged game still has the same winner.
    assert_general_matches_srf(&pbes);

    let mut normalised = pbes.clone();
    normalised.normalize();
    let game = explore_pbes(
        normalised,
        ExplorationStrategy::Bfs,
        &Timing::new(),
        ParityGameBuilder::new(VertexIndex::new(0)),
    )
    .expect("general exploration failed");

    // 4 instantiations + 1 shared disjunction + 2 sinks. It would be 8 if
    // the disjunction were split per priority.
    assert_eq!(
        game.num_of_vertices(),
        7,
        "the disjunction shared by X and W must be a single vertex"
    );
}

#[test]
fn test_parallel_random_pbes_seeds() {
    use merc_syntax::random_pbes;
    use rand::SeedableRng;

    for seed in 0u64..50 {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
        let pbes_ast = random_pbes(&mut rng, 0, 0, 3, 2, 3, false, false);
        let pbes = Pbes::from_text(&pbes_ast.to_string()).expect("parse failed");
        assert_parallel_matches_sequential(&pbes);
    }
}

#[test]
fn test_cached_random_pbes_seeds() {
    use merc_syntax::random_pbes;
    use rand::SeedableRng;

    for seed in 0u64..50 {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
        let pbes_ast = random_pbes(&mut rng, 0, 0, 3, 2, 3, false, false);
        let pbes = Pbes::from_text(&pbes_ast.to_string()).expect("parse failed");
        assert_cached_matches_uncached(&pbes);
    }
}

#[test]
fn test_random_pbes_seeds() {
    use merc_syntax::random_pbes;
    use rand::SeedableRng;

    for seed in 0u64..50 {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
        // propositional only (no quantifiers, no integers) — stable state spaces
        let pbes_ast = random_pbes(&mut rng, 0, 0, 3, 2, 3, false, false);
        let text = pbes_ast.to_string();
        assert_general_matches_srf_from_text(&text);
    }
}

fn convert_text_pbes_and_compare(text_pbes_path: &str) {
    let Ok(mcrl2_path) = std::env::var("MCRL2_PATH") else {
        println!("Skipping test: MCRL2_PATH not set");
        return;
    };

    let txt2pbes = Path::new(&mcrl2_path).join("txt2pbes");
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(text_pbes_path);
    assert!(path.exists(), "file not found: {}", path.display());

    let temp = temp_dir("test_explore_pbes").unwrap();
    let pbes_path = temp.path().join("spec.pbes");

    let status = traced_command(Command::new(&txt2pbes).arg(&path).arg(&pbes_path)).expect("txt2pbes failed");
    assert!(status.success());

    let pbes = Pbes::from_file(pbes_path.to_str().unwrap()).expect("Failed to read PBES");
    assert_general_matches_srf(&pbes);
}

fn convert_mcrl2_and_compare(spec: &str, formula: &str) {
    let Ok(mcrl2_path) = std::env::var("MCRL2_PATH") else {
        println!("Skipping test: MCRL2_PATH not set");
        return;
    };

    let mcrl22lps = Path::new(&mcrl2_path).join("mcrl22lps");
    let lps2pbes = Path::new(&mcrl2_path).join("lps2pbes");

    let spec_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(spec);
    let formula_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(formula);
    assert!(spec_path.exists(), "spec not found: {}", spec_path.display());
    assert!(formula_path.exists(), "formula not found: {}", formula_path.display());

    let temp = temp_dir("test_explore_pbes").unwrap();
    let lps_path = temp.path().join("spec.lps");
    let pbes_path = temp.path().join("spec.pbes");

    let status = traced_command(Command::new(&mcrl22lps).arg(&spec_path).arg(&lps_path)).expect("mcrl22lps failed");
    assert!(status.success());

    let status = traced_command(
        Command::new(&lps2pbes)
            .arg("-f")
            .arg(&formula_path)
            .arg(&lps_path)
            .arg(&pbes_path),
    )
    .expect("lps2pbes failed");
    assert!(status.success());

    let pbes = Pbes::from_file(pbes_path.to_str().unwrap()).expect("Failed to read PBES");
    assert_general_matches_srf(&pbes);
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_file_a_text_pbes() {
    convert_text_pbes_and_compare("../../../../examples/pbes/a.text.pbes");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_file_b_text_pbes() {
    convert_text_pbes_and_compare("../../../../examples/pbes/b.text.pbes");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_file_c_text_pbes() {
    convert_text_pbes_and_compare("../../../../examples/pbes/c.text.pbes");
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_file_par_nodeadlock() {
    convert_mcrl2_and_compare(
        "../../../../examples/mCRL2/academic/par/par.mcrl2",
        "../../../../examples/mCRL2/academic/par/nodeadlock.mcf",
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_file_abp() {
    convert_mcrl2_and_compare(
        "../../../../examples/mCRL2/academic/abp/abp.mcrl2",
        "../../../../examples/mCRL2/academic/abp/infinitely_often_enabled_then_infinitely_often_taken.mcf",
    );
}

#[test]
#[cfg_attr(miri, ignore)]
fn test_file_dining3_nodeadlock() {
    convert_mcrl2_and_compare(
        "../../../../examples/mCRL2/academic/dining/dining3.mcrl2",
        "../../../../examples/mCRL2/academic/dining/nodeadlock.mcf",
    );
}
