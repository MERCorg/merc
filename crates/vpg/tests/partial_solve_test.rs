use rand::RngExt;
use rand::SeedableRng;

use oxidd::ManagerRef;
use oxidd::ldd::LDDFunction;

use merc_io::DumpFiles;
use merc_utilities::random_test;

use merc_symbolic::LDD_CACHE_CAPACITY;
use merc_symbolic::LDD_NODE_CAPACITY;
use merc_vpg::AttractorProgress;
use merc_vpg::ExtendedParityGame;
use merc_vpg::PG;
use merc_vpg::ParityGameBuilder;
use merc_vpg::Player;
use merc_vpg::Priority;
use merc_vpg::RecursionProgress;
use merc_vpg::SymbolicSolution;
use merc_vpg::VertexIndex;
use merc_vpg::detect_fatal_attractors;
use merc_vpg::detect_fatal_attractors_within_safe_vertices;
use merc_vpg::detect_forced_cycles;
use merc_vpg::detect_forced_cycles_within_safe_vertices;
use merc_vpg::detect_solitair_cycles;
use merc_vpg::detect_solitair_cycles_within_safe_vertices;
use merc_vpg::encode_parity_game;
use merc_vpg::partial_solve;
use merc_vpg::random_parity_game;
use merc_vpg::solve_zielonka;
use merc_vpg::write_pg;

/// A progress tracker that never actually prints (interval far longer than any test run),
/// for tests that need one to pass but have nothing useful to say about it.
fn silent_attractor_progress() -> AttractorProgress {
    AttractorProgress::new(|_| {}, 3600)
}

/// See [`silent_attractor_progress`].
fn silent_recursion_progress() -> RecursionProgress {
    RecursionProgress::new(|_| {}, 3600)
}

/// Cross-checks [`partial_solve`] against the explicit [`solve_zielonka`] for *soundness* on
/// random total games, with `incomplete = ∅`: whichever vertices it does resolve must agree
/// with the oracle. It is not required to resolve everything — `incomplete = ∅` makes every
/// vertex *safe*, but `partial_solve` still returns as soon as `initial_vertex`'s winner is
/// decided (see its doc comment), so a real subset of `v` going unresolved is expected, not a
/// bug.
#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_partial_solve_is_sound() {
    random_test(100, |rng| {
        let game = random_parity_game(rng, true, 60, 5, 3);
        let (expected, _) = solve_zielonka(&game, false);

        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let radix = rng.random_range(2..=5);
        let num_groups = rng.random_range(1..=3);
        let (symbolic, all_vertices, cubes) =
            encode_parity_game(&manager, &game, rng, radix, num_groups, false).unwrap();

        let empty_sinks = symbolic.sinks(&all_vertices, &all_vertices).unwrap();
        let incomplete = manager.with_manager_shared(LDDFunction::empty_set).unwrap();
        let initial = manager
            .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[0]))
            .unwrap();

        let empty = manager.with_manager_shared(LDDFunction::empty_set).unwrap();
        let partial_solution = SymbolicSolution {
            winning: [empty.clone(), empty],
            strategy: [None, None],
        };

        let epg = ExtendedParityGame::new(symbolic, initial, empty_sinks);
        let solution = partial_solve(
            &epg,
            &incomplete,
            partial_solution,
            &silent_recursion_progress(),
            &silent_attractor_progress(),
        )
        .unwrap();

        let initial_winner = if expected[Player::Even.to_index()][0] {
            Player::Even
        } else {
            Player::Odd
        };
        assert_eq!(
            solution.winner(&epg.initial_vertex),
            Some(initial_winner),
            "initial vertex must be resolved"
        );

        for v in game.iter_vertices() {
            let vertex = manager
                .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[*v]))
                .unwrap();
            if let Some(winner) = solution.winner(&vertex) {
                let expected_winner = if expected[Player::Even.to_index()][*v] {
                    Player::Even
                } else {
                    Player::Odd
                };
                assert_eq!(winner, expected_winner, "vertex {v}");
            }
        }
    });
}

/// Cross-checks [`detect_solitair_cycles`], [`detect_forced_cycles`] and
/// [`detect_fatal_attractors`] (and their `_within_safe_vertices` counterparts) for
/// *soundness* against the explicit [`solve_zielonka`] on random total games: unlike
/// [`partial_solve`], these accelerators only ever resolve part of the game (a
/// solitair/forced/fatal cycle need not exist at all in a given random game), so the property
/// to check is that whatever they *do* claim agrees with the oracle — completeness is checked
/// separately, on hand-built fixtures guaranteed to contain a cycle, below.
#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_cycle_detectors_are_sound() {
    random_test(100, |rng| {
        let game = random_parity_game(rng, true, 60, 5, 3);
        let (expected, _) = solve_zielonka(&game, false);

        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let radix = rng.random_range(2..=5);
        let num_groups = rng.random_range(1..=3);
        let (symbolic, all_vertices, cubes) =
            encode_parity_game(&manager, &game, rng, radix, num_groups, false).unwrap();

        let empty_sinks = symbolic.sinks(&all_vertices, &all_vertices).unwrap();
        let incomplete = manager.with_manager_shared(LDDFunction::empty_set).unwrap();
        let initial = manager
            .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[0]))
            .unwrap();
        let empty = manager.with_manager_shared(LDDFunction::empty_set).unwrap();

        let epg = ExtendedParityGame::new(symbolic, initial, empty_sinks);

        let expected_winner = |v: usize| {
            if expected[Player::Even.to_index()][v] {
                Player::Even
            } else {
                Player::Odd
            }
        };

        let fresh_solution = || SymbolicSolution {
            winning: [empty.clone(), empty.clone()],
            strategy: [None, None],
        };

        // "safe" mode: folds `incomplete` directly into every attractor call.
        let solitair_safe =
            detect_solitair_cycles(&epg, &incomplete, fresh_solution(), &silent_attractor_progress()).unwrap();
        let forced_safe =
            detect_forced_cycles(&epg, &incomplete, fresh_solution(), &silent_attractor_progress()).unwrap();
        let fatal_safe =
            detect_fatal_attractors(&epg, &incomplete, &empty, &empty, &silent_attractor_progress()).unwrap();

        // "within_safe_vertices" mode: restricts the search up front to `safe_vertices`.
        let solitair_restricted = detect_solitair_cycles_within_safe_vertices(
            &epg,
            &incomplete,
            fresh_solution(),
            &silent_attractor_progress(),
        )
        .unwrap();
        let forced_restricted = detect_forced_cycles_within_safe_vertices(
            &epg,
            &incomplete,
            fresh_solution(),
            &silent_attractor_progress(),
        )
        .unwrap();
        let fatal_restricted = detect_fatal_attractors_within_safe_vertices(
            &epg,
            &incomplete,
            &empty,
            &empty,
            &silent_attractor_progress(),
        )
        .unwrap();

        for v in game.iter_vertices() {
            let vertex = manager
                .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[*v]))
                .unwrap();

            for (label, solitair, forced, fatal) in [
                ("safe", &solitair_safe, &forced_safe, &fatal_safe),
                (
                    "within_safe_vertices",
                    &solitair_restricted,
                    &forced_restricted,
                    &fatal_restricted,
                ),
            ] {
                if let Some(winner) = solitair.winner(&vertex) {
                    assert_eq!(winner, expected_winner(*v), "solitair: vertex {v}, mode={label}");
                }
                if let Some(winner) = forced.winner(&vertex) {
                    assert_eq!(winner, expected_winner(*v), "forced: vertex {v}, mode={label}");
                }
                for player in [Player::Even, Player::Odd] {
                    if merc_symbolic::element_of(&manager, &cubes[*v], &fatal[player.to_index()]) {
                        assert_eq!(player, expected_winner(*v), "fatal: vertex {v}, mode={label}");
                    }
                }
            }
        }
    });
}

/// Validate that with incomplete vertices the safe attractors are sound.
#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_partial_solving_is_sound_with_incomplete_vertices() {
    random_test(100, |rng| {
        let game = random_parity_game(rng, true, 60, 5, 3);
        let (expected, _) = solve_zielonka(&game, false);

        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let radix = rng.random_range(2..=5);
        let num_groups = rng.random_range(1..=3);
        let (symbolic, all_vertices, cubes) =
            encode_parity_game(&manager, &game, rng, radix, num_groups, false).unwrap();

        let empty_sinks = symbolic.sinks(&all_vertices, &all_vertices).unwrap();
        let initial = manager
            .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[0]))
            .unwrap();
        let empty = manager.with_manager_shared(LDDFunction::empty_set).unwrap();

        // A random subset of vertices, treated as if their outgoing edges were not yet learned —
        // even though `symbolic` actually has all of them already. Every accelerator must stay
        // sound against the real game regardless.
        let incomplete_cubes: Vec<_> = cubes.iter().filter(|_| rng.random_bool(0.3)).collect();
        let incomplete = merc_symbolic::from_iter(&manager, incomplete_cubes.into_iter());

        let epg = ExtendedParityGame::new(symbolic, initial, empty_sinks);
        let expected_winner = |v: usize| {
            if expected[Player::Even.to_index()][v] {
                Player::Even
            } else {
                Player::Odd
            }
        };
        let fresh_solution = || SymbolicSolution {
            winning: [empty.clone(), empty.clone()],
            strategy: [None, None],
        };

        let partial = partial_solve(
            &epg,
            &incomplete,
            fresh_solution(),
            &silent_recursion_progress(),
            &silent_attractor_progress(),
        )
        .unwrap();

        let solitair_safe =
            detect_solitair_cycles(&epg, &incomplete, fresh_solution(), &silent_attractor_progress()).unwrap();
        let solitair_restricted = detect_solitair_cycles_within_safe_vertices(
            &epg,
            &incomplete,
            fresh_solution(),
            &silent_attractor_progress(),
        )
        .unwrap();
        let forced_safe =
            detect_forced_cycles(&epg, &incomplete, fresh_solution(), &silent_attractor_progress()).unwrap();
        let forced_restricted = detect_forced_cycles_within_safe_vertices(
            &epg,
            &incomplete,
            fresh_solution(),
            &silent_attractor_progress(),
        )
        .unwrap();
        let fatal_safe =
            detect_fatal_attractors(&epg, &incomplete, &empty, &empty, &silent_attractor_progress()).unwrap();
        let fatal_restricted = detect_fatal_attractors_within_safe_vertices(
            &epg,
            &incomplete,
            &empty,
            &empty,
            &silent_attractor_progress(),
        )
        .unwrap();

        // `LDDFunction` has no `Debug` impl, so these can't be `assert_eq!`.
        assert!(
            solitair_safe.winning == solitair_restricted.winning,
            "detect_solitair_cycles: Safe and Restricted must agree exactly"
        );
        assert!(
            forced_safe.winning == forced_restricted.winning,
            "detect_forced_cycles: Safe and Restricted must agree exactly"
        );
        assert!(
            fatal_safe == fatal_restricted,
            "detect_fatal_attractors: Safe and Restricted must agree exactly"
        );

        for v in game.iter_vertices() {
            let vertex = manager
                .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[*v]))
                .unwrap();

            if let Some(winner) = partial.winner(&vertex) {
                assert_eq!(winner, expected_winner(*v), "partial_solve: vertex {v}");
            }
            if let Some(winner) = solitair_safe.winner(&vertex) {
                assert_eq!(winner, expected_winner(*v), "detect_solitair_cycles: vertex {v}");
            }
            if let Some(winner) = forced_safe.winner(&vertex) {
                assert_eq!(winner, expected_winner(*v), "detect_forced_cycles: vertex {v}");
            }
            for player in [Player::Even, Player::Odd] {
                if merc_symbolic::element_of(&manager, &cubes[*v], &fatal_safe[player.to_index()]) {
                    assert_eq!(player, expected_winner(*v), "detect_fatal_attractors: vertex {v}");
                }
            }
        }
    });
}

/// Exercises the strategy-computation code paths of [`partial_solve`], [`detect_solitair_cycles`]
/// and [`detect_forced_cycles`] (`compute_strategy = true`; [`detect_fatal_attractors`] never
/// computes one), which no other test in this crate reaches — every
/// `expect("compute_strategy is set")` in `partial_solve.rs`'s strategy bookkeeping is otherwise
/// dead code as far as the test suite is concerned. Only checks that computing a strategy doesn't
/// panic and that whatever comes back is accepted by [`merc_vpg::SymbolicParityGame::apply_strategy`];
/// a full certificate would need `verify_symbolic_strategy`, which requires the *whole* game to be
/// resolved, not just whatever these accelerators manage to resolve on a random game.
#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_partial_solving_computes_a_valid_strategy() {
    random_test(100, |rng| {
        let game = random_parity_game(rng, true, 60, 5, 3);

        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let radix = rng.random_range(2..=5);
        let num_groups = rng.random_range(1..=3);
        let (symbolic, all_vertices, cubes) =
            encode_parity_game(&manager, &game, rng, radix, num_groups, true).unwrap();

        let empty_sinks = symbolic.sinks(&all_vertices, &all_vertices).unwrap();
        let incomplete = manager.with_manager_shared(LDDFunction::empty_set).unwrap();
        let initial = manager
            .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[0]))
            .unwrap();
        let empty = manager.with_manager_shared(LDDFunction::empty_set).unwrap();

        let fresh_solution = || SymbolicSolution {
            winning: [empty.clone(), empty.clone()],
            strategy: [Some(empty.clone()), Some(empty.clone())],
        };

        let epg = ExtendedParityGame::new(symbolic, initial, empty_sinks);

        let solutions = [
            partial_solve(
                &epg,
                &incomplete,
                fresh_solution(),
                &silent_recursion_progress(),
                &silent_attractor_progress(),
            )
            .unwrap(),
            detect_solitair_cycles(&epg, &incomplete, fresh_solution(), &silent_attractor_progress()).unwrap(),
            detect_forced_cycles(&epg, &incomplete, fresh_solution(), &silent_attractor_progress()).unwrap(),
        ];

        for solution in solutions {
            for player in [Player::Even, Player::Odd] {
                let strategy = solution.strategy[player.to_index()]
                    .as_ref()
                    .expect("compute_strategy is set");
                epg.game
                    .apply_strategy(player, strategy)
                    .expect("a partial solver's own strategy must be a valid input to apply_strategy");
            }
        }
    });
}

/// A tiny fixed game with a genuine solitair cycle: vertex 0 (Even-owned, priority 0, so
/// `Player::from_priority` maps it to Even — matching its owner, exactly the coincidence
/// [`detect_solitair_cycles`] needs) has a self-loop, alongside an edge into vertex 1 that
/// leads nowhere useful for Even. Even can simply always choose the self-loop, so vertex 0
/// (and, by attraction, vertex 1, since Odd's only move from 1 is back to 0) must be won by
/// Even — checks completeness where the random test above only checks soundness.
fn solitair_cycle_fixture() -> merc_vpg::ParityGame {
    let mut builder = ParityGameBuilder::new(VertexIndex::new(0));
    builder.add_vertex(VertexIndex::new(0), Player::Even, Priority::new(0));
    builder.add_vertex(VertexIndex::new(1), Player::Odd, Priority::new(1));
    builder.add_edge(VertexIndex::new(0), VertexIndex::new(0));
    builder.add_edge(VertexIndex::new(0), VertexIndex::new(1));
    builder.add_edge(VertexIndex::new(1), VertexIndex::new(0));
    builder.finish(false, true)
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_detect_solitair_cycles_finds_the_fixture_cycle() {
    let files = DumpFiles::new("test_detect_solitair_cycles_finds_the_fixture_cycle");
    let game = solitair_cycle_fixture();
    files.dump("input.pg", |writer| write_pg(writer, &game)).unwrap();

    let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
    let mut rng = rand::rngs::SmallRng::seed_from_u64(0);
    let (symbolic, all_vertices, cubes) = encode_parity_game(&manager, &game, &mut rng, 2, 1, false).unwrap();

    let empty_sinks = symbolic.sinks(&all_vertices, &all_vertices).unwrap();
    let empty = manager.with_manager_shared(LDDFunction::empty_set).unwrap();
    let incomplete = empty.clone();
    let initial = manager
        .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[0]))
        .unwrap();

    let partial_solution = SymbolicSolution {
        winning: [empty.clone(), empty],
        strategy: [None, None],
    };

    let epg = ExtendedParityGame::new(symbolic, initial, empty_sinks);
    let solution =
        detect_solitair_cycles_within_safe_vertices(&epg, &incomplete, partial_solution, &silent_attractor_progress())
            .unwrap();

    for v in [0usize, 1] {
        let vertex = manager
            .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[v]))
            .unwrap();
        assert_eq!(
            solution.winner(&vertex),
            Some(Player::Even),
            "vertex {v} must be resolved (won by Even) by the solitair cycle at vertex 0"
        );
    }
}

/// A tiny fixed game with a genuine *forced* (but not solitair) cycle: vertex 0 is Odd-owned
/// with a *single* outgoing edge to vertex 1 (Even-owned), which has a single edge straight
/// back to vertex 0 — both at priority 0 (even). [`detect_forced_cycles`]'s seed is every
/// vertex sharing `alpha`'s own priority *parity*, regardless of owner, so both vertices seed
/// it; Odd has no choice at all (degree 1), so it is trivially "forced" to stay inside `{0,
/// 1}`, and the whole 2-cycle is won by Even (its only priority, 0, is even).
/// [`detect_solitair_cycles`] cannot find this: its seed is restricted to vertices `alpha`
/// itself *owns*, i.e. just `{1}` here, and `{1}` alone does not self-sustain (vertex 1's
/// only edge leaves it, to vertex 0) — only [`detect_forced_cycles`] can bring vertex 0's
/// forced, single-edge cooperation into the picture.
fn forced_cycle_fixture() -> merc_vpg::ParityGame {
    let mut builder = ParityGameBuilder::new(VertexIndex::new(0));
    builder.add_vertex(VertexIndex::new(0), Player::Odd, Priority::new(0));
    builder.add_vertex(VertexIndex::new(1), Player::Even, Priority::new(0));
    builder.add_edge(VertexIndex::new(0), VertexIndex::new(1));
    builder.add_edge(VertexIndex::new(1), VertexIndex::new(0));
    builder.finish(false, true)
}

#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_detect_forced_cycles_finds_the_fixture_cycle() {
    let files = DumpFiles::new("test_detect_forced_cycles_finds_the_fixture_cycle");
    let game = forced_cycle_fixture();
    files.dump("input.pg", |writer| write_pg(writer, &game)).unwrap();

    // Sanity check that this really is a forced-but-not-solitair cycle for the explicit
    // solver too, i.e. that the fixture tests what it claims to.
    let (expected, _) = solve_zielonka(&game, false);
    for v in [0usize, 1] {
        assert!(
            expected[Player::Even.to_index()][v],
            "fixture sanity check: vertex {v} should be Even's"
        );
    }

    let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
    let mut rng = rand::rngs::SmallRng::seed_from_u64(0);
    let (symbolic, all_vertices, cubes) = encode_parity_game(&manager, &game, &mut rng, 2, 1, false).unwrap();

    let empty_sinks = symbolic.sinks(&all_vertices, &all_vertices).unwrap();
    let empty = manager.with_manager_shared(LDDFunction::empty_set).unwrap();
    let incomplete = empty.clone();
    let initial = manager
        .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[0]))
        .unwrap();
    let epg = ExtendedParityGame::new(symbolic, initial, empty_sinks);

    // The solitair detector must not resolve anything here: vertex 0 (the only vertex at an
    // even priority owned by anyone) is owned by Odd, so it never seeds the search.
    let solitair_partial_solution = SymbolicSolution {
        winning: [empty.clone(), empty.clone()],
        strategy: [None, None],
    };
    let solitair = detect_solitair_cycles_within_safe_vertices(
        &epg,
        &incomplete,
        solitair_partial_solution,
        &silent_attractor_progress(),
    )
    .unwrap();
    for v in [0usize, 1] {
        let vertex = manager
            .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[v]))
            .unwrap();
        assert_eq!(
            solitair.winner(&vertex),
            None,
            "vertex {v} must NOT be resolved by the solitair detector (no vertex here owns its own escape)"
        );
    }

    let forced_partial_solution = SymbolicSolution {
        winning: [empty.clone(), empty],
        strategy: [None, None],
    };
    let forced = detect_forced_cycles_within_safe_vertices(
        &epg,
        &incomplete,
        forced_partial_solution,
        &silent_attractor_progress(),
    )
    .unwrap();
    for v in [0usize, 1] {
        let vertex = manager
            .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[v]))
            .unwrap();
        assert_eq!(
            forced.winner(&vertex),
            Some(Player::Even),
            "vertex {v} must be resolved (won by Even) by the forced cycle {{0, 1}}"
        );
    }
}

/// The same [`forced_cycle_fixture`] game, this time checking [`detect_fatal_attractors`]:
/// both vertices share the one priority (0, even), and vertex 0 can always be returned to, so
/// `{0, 1}` is a textbook fatal attractor for Even.
#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_detect_fatal_attractors_finds_the_fixture_cycle() {
    let game = forced_cycle_fixture();

    let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
    let mut rng = rand::rngs::SmallRng::seed_from_u64(0);
    let (symbolic, all_vertices, cubes) = encode_parity_game(&manager, &game, &mut rng, 2, 1, false).unwrap();

    let empty_sinks = symbolic.sinks(&all_vertices, &all_vertices).unwrap();
    let empty = manager.with_manager_shared(LDDFunction::empty_set).unwrap();
    let incomplete = empty.clone();
    let initial = manager
        .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[0]))
        .unwrap();
    let epg = ExtendedParityGame::new(symbolic, initial, empty_sinks);

    let winning =
        detect_fatal_attractors_within_safe_vertices(&epg, &incomplete, &empty, &empty, &silent_attractor_progress())
            .unwrap();

    for v in [0usize, 1] {
        assert!(
            merc_symbolic::element_of(&manager, &cubes[v], &winning[Player::Even.to_index()]),
            "vertex {v} must be won by Even via the fatal attractor {{0, 1}}"
        );
    }
}
