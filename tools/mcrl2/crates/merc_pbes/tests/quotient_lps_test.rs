//! Integration tests for [`QuotientLps`], exercising only its public API.

use std::sync::Arc;
use std::sync::OnceLock;

use mcrl2::Pbes;
use mcrl2::SrfPbes;
use merc_explore::CacheLPS;
use merc_explore::CachingStrategy;
use merc_explore::ExplorationStrategy;
use merc_utilities::MercError;
use merc_utilities::Timing;
use merc_utilities::random_test;
use merc_vpg::PG;
use merc_vpg::ParityGameBuilder;
use merc_vpg::VertexIndex;
use merc_vpg::solve_zielonka;

use merc_pbes::Bsgs;
use merc_pbes::Canonicaliser;
use merc_pbes::GapConfig;
use merc_pbes::PbesLps;
use merc_pbes::PbesSrfLps;
use merc_pbes::Permutation;
use merc_pbes::QuotientLps;
use merc_pbes::explore_pbes_impl;
use merc_pbes::symmetry_parameter_basis;

use mcrl2::DataRewriter;
use mcrl2::RewriteStrategy;
use rand::RngExt;
use rand::rngs::StdRng;
use rand::seq::SliceRandom;

fn gap_config() -> GapConfig {
    GapConfig {
        executable: "gap".to_string(),
        dump_script: None,
    }
}

/// Probe whether GAP is on the path, cached once per test process.
fn gap_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        std::process::Command::new("gap")
            .args(["-q", "-A", "--quitonbreak"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write as _;
                child
                    .stdin
                    .as_mut()
                    .expect("stdin was piped")
                    .write_all(b"QUIT_GAP(0);;")?;
                child.wait()
            })
            .map(|status| status.success())
            .unwrap_or(false)
    })
}

/// Build a fully symmetric `k`-parameter PBES over `Nat` values restricted to
/// `{0, 1}` (via `mod 2`). The transition relation is the disjunction over all
/// ways to toggle exactly `t` of the `k` parameters to their opposite value, so
/// it is invariant under the full symmetric group `S_k` (any relabelling of the
/// parameter slots maps one toggling move to another in the same set).
///
/// This means *any* subgroup of `S_k` is a genuine symmetry of the PBES, which
/// is what lets a random generator set combine with the quotient soundly.
fn symmetric_pbes(k: usize, t: usize) -> String {
    let params: Vec<String> = (0..k).map(|i| format!("v{i}: Nat")).collect();
    let params = params.join(", ");

    let init_args = vec!["0".to_string(); k].join(", ");

    // All subsets of exactly `t` toggled positions.
    let mut toggles: Vec<Vec<usize>> = Vec::new();
    let mut combo = (0..t).collect::<Vec<usize>>();
    loop {
        toggles.push(combo.clone());
        // Iterate over combinations of size t from 0..k.
        let mut i = t;
        while i > 0 && combo[i - 1] == k - t + i - 1 {
            i -= 1;
        }
        if i == 0 {
            break;
        }
        combo[i - 1] += 1;
        for j in i..t {
            combo[j] = combo[j - 1] + 1;
        }
    }

    // One `X(next)` disjunct per toggling move; toggled bits flip mod 2.
    let toggles_text: Vec<String> = toggles
        .iter()
        .map(|toggle| {
            let next: Vec<String> = (0..k)
                .map(|j| {
                    if toggle.contains(&j) {
                        format!("(v{j} + 1) mod 2")
                    } else {
                        format!("v{j}")
                    }
                })
                .collect();
            format!("X({})", next.join(", "))
        })
        .collect();

    let rhs = toggles_text.join(" || ");

    format!("pbes\nnu X({params}) = {rhs};\ninit X({init_args});")
}

/// Converts `pbes` to SRF form and unifies its parameter lists (with the same
/// flags `symmetry_parameter_basis` uses), as every SRF-based explorer
/// requires (see `PbesSrfLps::new`).
fn unified_srf(pbes: &Pbes) -> SrfPbes {
    let mut srf = SrfPbes::from(pbes).expect("Failed to convert to SRF");
    srf.unify_parameters(false, true).expect("Failed to unify parameters");
    srf
}

/// Verifies that wrapping `PbesSrfLps` in `QuotientLps` (with trivial group)
/// produces the same parity game size as the unwrapped LPS.
#[test]
#[cfg_attr(miri, ignore)]
fn quotient_trivial_group_same_game_size() -> Result<(), MercError> {
    let pbes_text = r#"pbes
nu X(b: Bool) = X(true);
init X(true);"#;
    let pbes = Pbes::from_text(pbes_text)?;

    let input_spec = unified_srf(&pbes);
    let rewriter = DataRewriter::new(&input_spec.data_specification(), RewriteStrategy::default())
        .expect("the rewriter should be built");
    let lps = PbesSrfLps::new(input_spec, rewriter)?;
    let n = lps.num_params();
    let bsgs = Arc::new(Bsgs::from_generators(&[], n, &gap_config())?);

    let timing = Timing::new();
    let plain_game = explore_pbes_impl(
        &lps,
        ExplorationStrategy::Bfs,
        &timing,
        ParityGameBuilder::new(VertexIndex::new(0)),
    )?;

    let input_spec = unified_srf(&pbes);
    let rewriter = DataRewriter::new(&input_spec.data_specification(), RewriteStrategy::default())
        .expect("the rewriter should be built");
    let lps2 = PbesSrfLps::new(input_spec, rewriter)?;
    let qlps = QuotientLps::new(lps2, Arc::new(Canonicaliser::Bsgs(bsgs)), 1);
    let quot_game = explore_pbes_impl(
        &qlps,
        ExplorationStrategy::Bfs,
        &timing,
        ParityGameBuilder::new(VertexIndex::new(0)),
    )?;

    assert_eq!(plain_game.num_of_vertices(), quot_game.num_of_vertices());
    assert_eq!(plain_game.num_of_edges(), quot_game.num_of_edges());
    Ok(())
}

/// Verifies that `QuotientLps<CacheLPS<PbesSrfLps>>` compiles and produces a
/// valid game (wrapping order: cache inside, quotient outside).
#[test]
#[cfg_attr(miri, ignore)]
fn quotient_with_cache_compiles() -> Result<(), MercError> {
    let pbes_text = r#"pbes
nu X(b: Bool) = X(true);
init X(true);"#;
    let pbes = Pbes::from_text(pbes_text)?;
    let input_spec = unified_srf(&pbes);
    let rewriter = DataRewriter::new(&input_spec.data_specification(), RewriteStrategy::default())
        .expect("the rewriter should be built");
    let lps = PbesSrfLps::new(input_spec, rewriter)?;
    let n = lps.num_params();
    let bsgs = Arc::new(Bsgs::from_generators(&[], n, &gap_config())?);

    let cached = CacheLPS::new(lps, CachingStrategy::Local);
    let qlps = QuotientLps::new(cached, Arc::new(Canonicaliser::Bsgs(bsgs)), 1);

    let timing = Timing::new();
    let game = explore_pbes_impl(
        &qlps,
        ExplorationStrategy::Bfs,
        &timing,
        ParityGameBuilder::new(VertexIndex::new(0)),
    )?;
    assert!(game.num_of_vertices() > 0);
    Ok(())
}

/// A PBES whose two parameters are interchangeable, explored with the general
/// (non-SRF) explorer so the game also contains sink and subformula vertices.
///
/// Those carry a priority and an interned formula index where a propositional
/// variable instantiation carries parameters, so a quotient that permutes them
/// unconditionally corrupts them — the interned index in particular becomes a
/// formula that does not exist.
const SYMMETRIC_PBES: &str = r#"pbes
nu X(m: Nat, n: Nat) = X(m, n) && (Y(n, m) || Y((m + 1) mod 2, n));
mu Y(m: Nat, n: Nat) = X(m, n) || Y((n + 1) mod 2, m);
init X(0, 1);"#;

#[test]
#[cfg_attr(miri, ignore)]
fn quotient_preserves_winner_and_reduces_the_game() -> Result<(), MercError> {
    let pbes = Pbes::from_text(SYMMETRIC_PBES)?;
    let timing = Timing::new();

    let plain = explore_pbes_impl(
        &PbesLps::new(
            pbes.clone(),
            DataRewriter::new(&pbes.data_specification(), RewriteStrategy::default())
                .expect("the rewriter should be built"),
        )?,
        ExplorationStrategy::Bfs,
        &timing,
        ParityGameBuilder::new(VertexIndex::new(0)),
    )?;

    // `n` sizes the permutation group; the tool itself derives this same degree
    // from `symmetry_parameter_basis` rather than from a constructed LPS.
    let n = symmetry_parameter_basis(&pbes)?.len();
    let rewriter = DataRewriter::new(&pbes.data_specification(), RewriteStrategy::default())
        .expect("the rewriter should be built");
    let lps = PbesLps::new(pbes, rewriter)?;
    let generators = vec![Permutation::from_cycle_notation("(0 1)")?];
    let bsgs = Arc::new(Bsgs::from_generators(&generators, n, &gap_config())?);
    let quotient = explore_pbes_impl(
        &QuotientLps::new(lps, Arc::new(Canonicaliser::Bsgs(bsgs)), 1),
        ExplorationStrategy::Bfs,
        &timing,
        ParityGameBuilder::new(VertexIndex::new(0)),
    )?;

    assert!(
        quotient.num_of_vertices() < plain.num_of_vertices(),
        "swapping the two parameters is a symmetry, so the quotient must be smaller \
         (plain {} vertices, quotient {})",
        plain.num_of_vertices(),
        quotient.num_of_vertices()
    );

    let (plain_solution, _) = solve_zielonka(&plain, false);
    let (quotient_solution, _) = solve_zielonka(&quotient, false);
    assert_eq!(
        plain_solution[0][0], quotient_solution[0][0],
        "the quotient changed the winner of the initial vertex"
    );
    Ok(())
}

/// The SRF backend must reduce and preserve the winner just like the general
/// one — `--srf --symmetry` layers the quotient over [`PbesSrfLps`].
///
/// The two games are not the same size: SRF normalisation rewrites the
/// right-hand sides, so this compares the SRF game against its own quotient
/// rather than against the general explorer's.
#[test]
#[cfg_attr(miri, ignore)]
fn quotient_over_srf_preserves_winner_and_reduces_the_game() -> Result<(), MercError> {
    let pbes = Pbes::from_text(SYMMETRIC_PBES)?;
    let timing = Timing::new();

    let plain = explore_pbes_impl(
        &PbesSrfLps::new(
            unified_srf(&pbes),
            DataRewriter::new(&pbes.data_specification(), RewriteStrategy::default())
                .expect("the rewriter should be built"),
        )?,
        ExplorationStrategy::Bfs,
        &timing,
        ParityGameBuilder::new(VertexIndex::new(0)),
    )?;

    let input_spec = unified_srf(&pbes);
    let rewriter = DataRewriter::new(&input_spec.data_specification(), RewriteStrategy::default())
        .expect("the rewriter should be built");
    let lps = PbesSrfLps::new(input_spec, rewriter)?;
    assert_eq!(
        lps.num_params(),
        2,
        "the generator below is a transposition of the whole parameter vector"
    );
    // Both parameters have the same sort, so the transposition is `(0 1)`
    // whichever of the two orders `unify_parameters` happened to produce.
    let generators = vec![Permutation::from_cycle_notation("(0 1)")?];
    let bsgs = Arc::new(Bsgs::from_generators(&generators, lps.num_params(), &gap_config())?);
    let quotient = explore_pbes_impl(
        &QuotientLps::new(lps, Arc::new(Canonicaliser::Bsgs(bsgs)), 1),
        ExplorationStrategy::Bfs,
        &timing,
        ParityGameBuilder::new(VertexIndex::new(0)),
    )?;

    assert!(
        quotient.num_of_vertices() < plain.num_of_vertices(),
        "swapping the two parameters is a symmetry, so the quotient must be smaller \
         (plain {} vertices, quotient {})",
        plain.num_of_vertices(),
        quotient.num_of_vertices()
    );

    let (plain_solution, _) = solve_zielonka(&plain, false);
    let (quotient_solution, _) = solve_zielonka(&quotient, false);
    assert_eq!(
        plain_solution[0][0], quotient_solution[0][0],
        "the quotient changed the winner of the initial vertex"
    );
    Ok(())
}

/// The same reduction must survive a cache layer underneath the quotient.
#[test]
#[cfg_attr(miri, ignore)]
fn quotient_over_cache_agrees_with_quotient_alone() -> Result<(), MercError> {
    let pbes = Pbes::from_text(SYMMETRIC_PBES)?;
    let generators = vec![Permutation::from_cycle_notation("(0 1)")?];
    let timing = Timing::new();

    // `n` sizes the permutation group; the tool itself derives this same degree
    // from `symmetry_parameter_basis` rather than from a constructed LPS.
    let n = symmetry_parameter_basis(&pbes)?.len();
    let input_spec = pbes.clone();
    let rewriter = DataRewriter::new(&input_spec.data_specification(), RewriteStrategy::default())
        .expect("the rewriter should be built");
    let lps = PbesLps::new(input_spec, rewriter)?;
    let bsgs = Arc::new(Bsgs::from_generators(&generators, n, &gap_config())?);
    let uncached = explore_pbes_impl(
        &QuotientLps::new(lps, Arc::new(Canonicaliser::Bsgs(Arc::clone(&bsgs))), 1),
        ExplorationStrategy::Bfs,
        &timing,
        ParityGameBuilder::new(VertexIndex::new(0)),
    )?;

    let rewriter = DataRewriter::new(&pbes.data_specification(), RewriteStrategy::default())
        .expect("the rewriter should be built");
    let cached = CacheLPS::new(PbesLps::new(pbes, rewriter)?, CachingStrategy::Local);
    let cached = explore_pbes_impl(
        &QuotientLps::new(cached, Arc::new(Canonicaliser::Bsgs(bsgs)), 1),
        ExplorationStrategy::Bfs,
        &timing,
        ParityGameBuilder::new(VertexIndex::new(0)),
    )?;

    assert_eq!(uncached.num_of_vertices(), cached.num_of_vertices());
    assert_eq!(uncached.num_of_edges(), cached.num_of_edges());
    Ok(())
}

/// The two [`Canonicaliser`] variants — the in-process [`Bsgs`] chain and the
/// persistent GAP lex-min session — must compute exactly the same quotient for
/// every next state, and both must reduce the game whenever the (random,
/// non-trivial) symmetry group actually merges states.
///
/// Skipped entirely when GAP is not on the path.
#[test]
#[cfg_attr(miri, ignore)]
fn random_quotient_bsgs_matches_gap_lexmin() {
    if !gap_available() {
        eprintln!("skipping: GAP is not available");
        return;
    }

    random_test(4, |rng| {
        let k = rng.random_range(2..=3usize);
        // Toggle exactly one parameter per move: starting from all-zero this
        // reaches every vector in {0,1}^k, so any non-trivial subgroup of S_k
        // merges at least one pair of reachable states.
        let t = 1;

        let pbes_text = symmetric_pbes(k, t);
        let pbes = Pbes::from_text(&pbes_text).expect("generated PBES must parse");
        let timing = Timing::new();

        // `n` sizes the permutation group, matching what the tool derives from
        // `symmetry_parameter_basis` for a multi-parameter PBES.
        let n = symmetry_parameter_basis(&pbes).expect("parameter basis").len();

        // A random non-trivial generator set of S_k. Any subgroup is a symmetry
        // because the PBES is fully S_k-symmetric.
        let num_gens = rng.random_range(1..=2);
        let generators: Vec<Permutation> = (0..num_gens).map(|_| random_non_identity(rng, k)).collect();

        let plain = explore_pbes_impl(
            &PbesLps::new(
                pbes.clone(),
                DataRewriter::new(&pbes.data_specification(), RewriteStrategy::default())
                    .expect("the rewriter should be built"),
            )
            .expect("PbesLps"),
            ExplorationStrategy::Bfs,
            &timing,
            ParityGameBuilder::new(VertexIndex::new(0)),
        )
        .expect("plain exploration");

        let bsgs = Arc::new(Bsgs::from_generators(&generators, n, &gap_config()).expect("BSGS"));
        let quot_bsgs = explore_pbes_impl(
            &QuotientLps::new(
                PbesLps::new(
                    pbes.clone(),
                    DataRewriter::new(&pbes.data_specification(), RewriteStrategy::default())
                        .expect("the rewriter should be built"),
                )
                .expect("PbesLps"),
                Arc::new(Canonicaliser::Bsgs(Arc::clone(&bsgs))),
                1,
            ),
            ExplorationStrategy::Bfs,
            &timing,
            ParityGameBuilder::new(VertexIndex::new(0)),
        )
        .expect("BSGS quotient");

        let rewriter = DataRewriter::new(&pbes.data_specification(), RewriteStrategy::default())
            .expect("the rewriter should be built");
        let quot_gap = explore_pbes_impl(
            &QuotientLps::new(
                PbesLps::new(pbes, rewriter).expect("PbesLps"),
                Arc::new(Canonicaliser::gap_lexmin(generators.clone(), n, &gap_config())),
                1,
            ),
            ExplorationStrategy::Bfs,
            &timing,
            ParityGameBuilder::new(VertexIndex::new(0)),
        )
        .expect("GAP lex-min quotient");

        assert_eq!(
            quot_bsgs.num_of_vertices(),
            quot_gap.num_of_vertices(),
            "k={k}, t={t}, |G|={}, generators {generators:?}",
            bsgs.order(),
        );
        assert_eq!(
            quot_bsgs.num_of_edges(),
            quot_gap.num_of_edges(),
            "k={k}, t={t}, |G|={}, generators {generators:?}",
            bsgs.order(),
        );

        let (bsgs_solution, _) = solve_zielonka(&quot_bsgs, false);
        let (gap_solution, _) = solve_zielonka(&quot_gap, false);
        assert_eq!(
            bsgs_solution[0][0], gap_solution[0][0],
            "the two variants disagree on the winner (k={k}, t={t}, generators {generators:?})"
        );

        assert!(
            quot_bsgs.num_of_vertices() < plain.num_of_vertices(),
            "a non-trivial symmetry must reduce the game (plain {} vertices, quotient {}, k={k}, generators {generators:?})",
            plain.num_of_vertices(),
            quot_bsgs.num_of_vertices(),
        );
    });
}

/// A uniformly random non-identity permutation of degree `k`.
fn random_non_identity(rng: &mut StdRng, k: usize) -> Permutation {
    loop {
        let mut image: Vec<usize> = (0..k).collect();
        image.shuffle(rng);
        let mapping: Vec<(usize, usize)> = (0..k).zip(image).filter(|(x, y)| x != y).collect();
        if !mapping.is_empty() {
            return Permutation::from_mapping(mapping);
        }
    }
}
