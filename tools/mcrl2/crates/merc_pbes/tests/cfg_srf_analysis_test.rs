//! Regression tests for the control flow graph analysis
//! ([`mcrl2::ControlFlowGraph`]) as layered onto SRF exploration by
//! [`CfgPbesSrfLps`], complementing the end-to-end checks in
//! `cfg_srf_test.rs`.
//!
//! `cfg_srf_test.rs` checks that pruning never changes the explored parity
//! game, and that a genuine control flow parameter (CFP) is found. The tests
//! here instead pin down when a parameter must *not* be classified as a CFP:
//! a bug that made the analysis too permissive would prune summands that can
//! actually fire, silently corrupting the parity game rather than merely
//! losing pruning opportunity - so the negative cases matter as much as the
//! positive one.

use mcrl2::Pbes;
use mcrl2::SrfPbes;
use merc_explore::CachingStrategy;
use merc_explore::ExplorationStrategy;
use merc_utilities::Timing;
use merc_vpg::PG;
use merc_vpg::ParityGameBuilder;
use merc_vpg::VertexIndex;
use merc_vpg::solve_zielonka;

use merc_pbes::CfgPbesSrfLps;
use merc_pbes::explore_srf_pbes;
use merc_pbes::explore_srf_pbes_parallel;

/// Converts `pbes` to SRF form and unifies its parameter lists, as every
/// SRF-based explorer requires (see `PbesSrfLps::new`).
fn unified_srf(pbes: &Pbes) -> SrfPbes {
    let mut srf = SrfPbes::from(pbes).expect("Failed to convert to SRF");
    srf.unify_parameters(false, true).expect("Failed to unify parameters");
    srf
}

/// Parses, normalises and converts `text` into a [`CfgPbesSrfLps`].
fn cfg_srf(text: &str) -> CfgPbesSrfLps {
    let mut pbes = Pbes::from_text(text).expect("Failed to parse PBES");
    pbes.normalize();
    CfgPbesSrfLps::new(unified_srf(&pbes)).expect("Failed to build control-flow SRF view")
}

/// Explores `text` (parsed and normalised) both with the plain SRF explorer
/// and with the control-flow-pruning variant, and asserts the two parity games
/// have equal vertex/edge counts and agree on the winner of the initial
/// vertex. Pruning summands whose source-value condition cannot hold must
/// never change the explored parity game — the decisive safety property that
/// a `control_flow_parameters()`/edge assertion alone cannot establish.
fn assert_cfg_matches_srf_text(text: &str) {
    let mut pbes = Pbes::from_text(text).expect("Failed to parse PBES");
    pbes.normalize();

    let reference = explore_srf_pbes(
        unified_srf(&pbes),
        ExplorationStrategy::Bfs,
        CachingStrategy::None,
        false,
        &Timing::new(),
        ParityGameBuilder::new(VertexIndex::new(0)),
    )
    .expect("SRF exploration failed");

    let cfg = explore_srf_pbes(
        unified_srf(&pbes),
        ExplorationStrategy::Bfs,
        CachingStrategy::None,
        true,
        &Timing::new(),
        ParityGameBuilder::new(VertexIndex::new(0)),
    )
    .expect("Control flow SRF exploration failed");

    assert_eq!(
        reference.num_of_vertices(),
        cfg.num_of_vertices(),
        "Vertex count mismatch between plain and control-flow-pruned SRF exploration"
    );
    assert_eq!(
        reference.num_of_edges(),
        cfg.num_of_edges(),
        "Edge count mismatch between plain and control-flow-pruned SRF exploration"
    );

    let (reference_solution, _) = solve_zielonka(&reference, false);
    let (cfg_solution, _) = solve_zielonka(&cfg, false);
    assert_eq!(
        reference_solution[0][0], cfg_solution[0][0],
        "Control-flow-pruned and plain SRF parity games disagree on the initial vertex's winner"
    );
}

/// A summand that changes a parameter to a constant *without* also guarding
/// its source value does not disqualify that parameter, as long as the write
/// is still to a closed value: pruning only ever uses a *reader* summand's own
/// `d == c` guard against the live current state, so an unconditional reset
/// elsewhere cannot make that unsound (see [`mcrl2::ControlFlowGraph`]'s
/// doc comment). Here `X`'s summand sets `s := 1` unconditionally (no guard on
/// `s`), yet `s` is still recognised as a CFP because `Y`'s summand guards it.
///
/// This matters in practice, not just in principle: mCRL2's SRF conversion
/// always emits boilerplate sink equations for the `true`/`false` PVIs whose
/// single summand resets every parameter unconditionally, and those sinks are
/// routinely reachable from real modal-formula translations (e.g. via a
/// `val(b) || X(...)` disjunct) — requiring every writer to also self-guard
/// would leave essentially every real PBES-in-SRF parameter disqualified.
#[test]
fn parameter_changed_without_a_source_guard_is_still_a_control_flow_parameter() {
    let text = "pbes mu X(s: Nat, b: Bool) =
                   val(!b) && Y(1, b);
                 mu Y(s: Nat, b: Bool) =
                   val(s == 1) && X(0, b);
             init X(0, false);";

    let lps = cfg_srf(text);
    assert_eq!(
        lps.control_flow_parameters().len(),
        1,
        "`Y`'s summand guards `s`, so it must be found as a CFP even though `X`'s summand resets \
         it unconditionally"
    );

    // The decisive check: pruning based on that classification must still
    // explore exactly the same parity game as without control-flow pruning.
    assert_cfg_matches_srf_text(text);
}

/// Regression test for the actual mCRL2 boilerplate case, not just a
/// hand-simulated stand-in for it: a bare `val(b)` disjunct (as opposed to
/// `val(b) && Y(...)`, which pairs a data condition with a target and needs no
/// sink) forces SRF conversion to route through its `true`/`false` sink
/// equation.
#[test]
fn control_flow_parameter_survives_a_reachable_true_false_sink() {
    let text = "pbes mu X(s: Nat, b: Bool) =
                   (val(s == 0) && val(b) && Y(1, false)) ||
                   (val(s == 1) && val(!b) && X(0, true));
                 mu Y(s: Nat, b: Bool) =
                   val(s == 1) && (val(b) || X(0, b));
             init X(0, false);";

    let lps = cfg_srf(text);
    assert_eq!(
        lps.control_flow_parameters().len(),
        1,
        "`s` must be found as a CFP even though Y's `val(b) || X(0, b)` disjunct routes through \
         a reachable `true`/`false` SRF sink"
    );

    assert_cfg_matches_srf_text(text);
}

/// Checks that `s` is still found as a CFP when `unify_parameters` runs with
/// `reset = false`.
#[test]
fn control_flow_parameter_survives_a_reachable_sink_without_reset() {
    let text = "pbes mu X(s: Nat, b: Bool) =
                   (val(s == 0) && val(b) && Y(1, false)) ||
                   (val(s == 1) && val(!b) && X(0, true));
                 mu Y(s: Nat, b: Bool) =
                   val(s == 1) && (val(b) || X(0, b));
             init X(0, false);";

    let mut pbes = Pbes::from_text(text).expect("Failed to parse PBES");
    pbes.normalize();
    let mut srf = SrfPbes::from(&pbes).expect("Failed to convert to SRF");
    srf.unify_parameters(false, false).expect("Failed to unify parameters");
    let lps = CfgPbesSrfLps::new(srf).expect("Failed to build control-flow SRF view");

    assert_eq!(
        lps.control_flow_parameters().len(),
        1,
        "`s` must still be found as a CFP when the reachable sink's undeclared parameters are \
         copied through (`s := s`) rather than reset to a default"
    );
}

/// A parameter that is written to constants everywhere but never appears in
/// any guard has no pruning opportunity to offer, so it must not be a CFP:
/// [`mcrl2::ControlFlowGraph`] still requires at least one live summand to
/// actually constrain it (`constrained_somewhere`) — the write/read relaxation
/// above only drops the requirement that the *same* summand do both.
#[test]
fn parameter_never_read_in_a_guard_is_not_a_control_flow_parameter() {
    let lps = cfg_srf(
        "pbes mu X(s: Nat, b: Bool) =
               (val(b) && X(1, !b)) ||
               (val(!b) && X(0, !b));
         init X(0, false);",
    );

    assert_eq!(
        lps.control_flow_parameters(),
        &[] as &[usize],
        "`s` is never read by a guard anywhere, so it must not be a CFP"
    );
}

/// A parameter that is assigned a non-constant expression (`s + 1`, which
/// stays open because SRF summands carry no substitution for their own
/// parameters) is not a control flow parameter, even though every summand
/// that changes it also guards it.
#[test]
fn parameter_changed_to_a_non_constant_value_is_not_a_control_flow_parameter() {
    let lps = cfg_srf(
        "pbes mu X(s: Nat, b: Bool) =
               val(s == 0) && Y(s + 1, b);
             mu Y(s: Nat, b: Bool) =
               val(s == 1) && X(0, b);
         init X(0, false);",
    );

    assert_eq!(
        lps.control_flow_parameters(),
        &[] as &[usize],
        "`s + 1` is not closed, so `s` must not be a CFP"
    );
}

/// A disjunctive guard `s == 0 || s == 1` does not register as a source
/// constraint on `s`: [`mcrl2::ControlFlowGraph`] only ever flattens `&&`,
/// so `X`'s summand below contributes no source constraint of its own. That no
/// longer disqualifies `s` from being a CFP (writers need not also be readers,
/// see `parameter_changed_without_a_source_guard_is_still_a_control_flow_parameter`)
/// since `X`'s write is still to a closed constant and `Y`'s guard constrains
/// `s` elsewhere — but it does mean `X`'s own summand gets no pruning benefit
/// from `s`: exploring it must still behave identically with or without
/// control-flow pruning, which `assert_cfg_matches_srf_text` is what actually
/// exercises that (`control_flow_parameters` alone can't tell the two apart).
#[test]
fn disjunctive_guard_does_not_count_as_a_source_constraint() {
    let text = "pbes mu X(s: Nat, b: Bool) =
                   (val(s == 0) || val(s == 1)) && Y(2, b);
                 mu Y(s: Nat, b: Bool) =
                   val(s == 2) && X(0, b);
             init X(0, false);";

    let lps = cfg_srf(text);
    assert_eq!(
        lps.control_flow_parameters().len(),
        1,
        "`s` is still a CFP: both writes are closed constants and `Y` constrains it"
    );

    assert_cfg_matches_srf_text(text);
}

/// The equality may be written with the constant on either side (`s == c` or
/// `c == s`); both must be recognised as the same kind of source constraint.
#[test]
fn source_constraint_is_recognised_regardless_of_equality_order() {
    let lps = cfg_srf(
        "pbes mu X(s: Nat, b: Bool) =
               val(0 == s) && Y(1, b);
             mu Y(s: Nat, b: Bool) =
               val(1 == s) && X(0, !b);
         init X(0, false);",
    );

    assert_eq!(
        lps.control_flow_parameters().len(),
        1,
        "`c == s` must be recognised as a source constraint just like `s == c`"
    );
}

/// Two independent parameters can each be genuine control flow parameters at
/// once; the analysis must not stop after finding the first one, and their
/// source constraints must be attributed to the right position.
#[test]
fn two_independent_control_flow_parameters_are_both_found() {
    let lps = cfg_srf(
        "pbes mu X(s: Nat, t: Nat) =
               (val(s == 0) && val(t == 0) && X(1, 1)) ||
               (val(s == 0) && val(t == 1) && X(1, 0)) ||
               (val(s == 1) && val(t == 0) && X(0, 1)) ||
               (val(s == 1) && val(t == 1) && X(0, 0));
         init X(0, 0);",
    );

    assert_eq!(
        lps.control_flow_parameters().len(),
        2,
        "both `s` and `t` behave as control flow parameters and must both be found"
    );
}

/// Explores `pbes` with control-flow pruning both sequentially and with the
/// parallel BFS, and asserts the two agree on the winner of the initial
/// vertex. [`explore_srf_pbes_parallel`]'s `control_flow` path is not
/// exercised by `cfg_srf_test.rs`, which only compares sequential runs.
fn assert_cfg_parallel_matches_sequential(text: &str) {
    let mut pbes = Pbes::from_text(text).expect("Failed to parse PBES");
    pbes.normalize();

    let sequential = explore_srf_pbes(
        unified_srf(&pbes),
        ExplorationStrategy::Bfs,
        CachingStrategy::None,
        true,
        &Timing::new(),
        ParityGameBuilder::new(VertexIndex::new(0)),
    )
    .expect("Sequential control-flow exploration failed");
    let (sequential_solution, _) = solve_zielonka(&sequential, false);

    for caching in [CachingStrategy::None, CachingStrategy::Local] {
        let parallel = explore_srf_pbes_parallel(
            unified_srf(&pbes),
            4,
            caching,
            true,
            false,
            &Timing::new(),
            ParityGameBuilder::new(VertexIndex::new(0)),
        )
        .expect("Parallel control-flow exploration failed");
        let (parallel_solution, _) = solve_zielonka(&parallel, false);

        assert_eq!(
            sequential_solution[0][0], parallel_solution[0][0],
            "parallel and sequential control-flow-pruned exploration disagree (caching = {caching:?})"
        );
    }
}

#[test]
fn control_flow_parallel_matches_sequential() {
    assert_cfg_parallel_matches_sequential(
        "pbes mu X(s: Nat, b: Bool) =
               (val(s == 0) && Y(1, b)) ||
               (val(s == 1) && X(0, !b));
             mu Y(s: Nat, b: Bool) =
               val(s == 1) && X(0, b);
         init X(0, false);",
    );
}

/// Fuzzes control-flow-pruned exploration under [`CachingStrategy::Local`]
/// against the uncached run, across the same seeds the other `random_pbes`
/// fuzz tests in this crate use.
#[test]
fn control_flow_matches_under_caching() {
    use merc_syntax::random_pbes;
    use rand::SeedableRng;

    for seed in 0u64..50 {
        let mut rng = rand::rngs::SmallRng::seed_from_u64(seed);
        let pbes_ast = random_pbes(&mut rng, 0, 0, 3, 2, 3, false, false);
        let mut pbes = Pbes::from_text(&pbes_ast.to_string()).expect("parse failed");
        pbes.normalize();

        let uncached = explore_srf_pbes(
            unified_srf(&pbes),
            ExplorationStrategy::Bfs,
            CachingStrategy::None,
            true,
            &Timing::new(),
            ParityGameBuilder::new(VertexIndex::new(0)),
        )
        .expect("uncached control-flow exploration failed");
        let cached = explore_srf_pbes(
            unified_srf(&pbes),
            ExplorationStrategy::Bfs,
            CachingStrategy::Local,
            true,
            &Timing::new(),
            ParityGameBuilder::new(VertexIndex::new(0)),
        )
        .expect("cached control-flow exploration failed");

        assert_eq!(
            uncached.num_of_vertices(),
            cached.num_of_vertices(),
            "caching changed the number of vertices under control-flow pruning (seed {seed})"
        );
        assert_eq!(
            uncached.num_of_edges(),
            cached.num_of_edges(),
            "caching changed the number of edges under control-flow pruning (seed {seed})"
        );
    }
}
