use rand::RngExt;

use merc_io::DumpFiles;
use merc_symbolic::element_of;
use merc_utilities::random_test;

use merc_symbolic::LDD_CACHE_CAPACITY;
use merc_symbolic::LDD_NODE_CAPACITY;
use merc_vpg::AttractorProgress;
use merc_vpg::PG;
use merc_vpg::Player;
use merc_vpg::VertexIndex;
use merc_vpg::convert_symbolic_parity_game;
use merc_vpg::encode_parity_game;
use merc_vpg::random_parity_game;
use merc_vpg::write_pg;

/// A progress tracker that never actually prints (interval far longer than any test run), for
/// tests that need one to pass but have nothing useful to say about it.
fn silent_attractor_progress() -> AttractorProgress {
    AttractorProgress::new(|_| {}, 3600)
}

/// Round-trips a random explicit parity game through the symbolic encoding and the
/// [`merc_vpg::convert_symbolic_parity_game`] oracle, checking that owners,
/// priorities and edges all survive.
#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_random_symbolic_game_round_trip() {
    random_test(100, |rng| {
        let files = DumpFiles::new("test_random_symbolic_game_round_trip");
        let game = random_parity_game(rng, true, 40, 5, 3);
        files.dump("input.pg", |writer| write_pg(writer, &game)).unwrap();

        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let radix = rng.random_range(2..=5);
        let num_groups = rng.random_range(1..=3);
        let (symbolic, all_vertices, cubes) =
            encode_parity_game(&manager, &game, rng, radix, num_groups, false).unwrap();

        let (decoded, decoded_cubes) = convert_symbolic_parity_game(&manager, &symbolic, &all_vertices).unwrap();

        assert_eq!(decoded.num_of_vertices(), game.num_of_vertices());
        assert_eq!(decoded.num_of_edges(), game.num_of_edges());

        for v in game.iter_vertices() {
            let decoded_index = decoded_cubes
                .iter()
                .position(|cube| *cube == cubes[*v])
                .expect("every encoded vertex must round-trip through the converter");
            let decoded_vertex = VertexIndex::new(decoded_index);

            assert_eq!(decoded.owner(decoded_vertex), game.owner(v), "vertex {v}");
            assert_eq!(decoded.priority(decoded_vertex), game.priority(v), "vertex {v}");

            let expected_successors: std::collections::HashSet<_> =
                game.outgoing_edges(v).map(|e| cubes[*e.to()].clone()).collect();
            let actual_successors: std::collections::HashSet<_> = decoded
                .outgoing_edges(decoded_vertex)
                .map(|e| decoded_cubes[*e.to()].clone())
                .collect();
            assert_eq!(actual_successors, expected_successors, "vertex {v}");
        }
    });
}

/// The vertices with no outgoing edge in a non-total random game are exactly the sinks
/// reported by [`merc_vpg::SymbolicParityGame::sinks`].
#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_random_symbolic_game_sinks_match_explicit_deadlocks() {
    random_test(100, |rng| {
        let game = random_parity_game(rng, false, 40, 5, 3);

        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let radix = rng.random_range(2..=5);
        let num_groups = rng.random_range(1..=3);
        let (symbolic, all_vertices, cubes) =
            encode_parity_game(&manager, &game, rng, radix, num_groups, false).unwrap();

        let sinks = symbolic.sinks(&all_vertices, &all_vertices).unwrap();

        for v in game.iter_vertices() {
            let is_explicit_deadlock = game.outgoing_edges(v).next().is_none();
            let is_symbolic_sink = element_of(&manager, &cubes[*v], &sinks);
            assert_eq!(is_symbolic_sink, is_explicit_deadlock, "vertex {v}");
        }
    });
}

/// [`merc_vpg::SymbolicParityGame::attractor_naive`] drops the incremental `todo` frontier that
/// [`merc_vpg::SymbolicParityGame::attractor`] uses, but the two are meant to compute exactly the
/// same least fixpoint — this checks that holds across random games, seeds and owning players,
/// not just that both terminate.
///
/// The Strategies do not have to be identical.
#[test]
#[cfg_attr(miri, ignore)] // Oxidd does not work with miri
fn test_attractor_naive_matches_attractor() {
    random_test(100, |rng| {
        let game = random_parity_game(rng, true, 40, 5, 3);

        let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
        let radix = rng.random_range(2..=5);
        let num_groups = rng.random_range(1..=3);
        let compute_strategy = rng.random_bool(0.5);
        let (symbolic, all_vertices, _cubes) =
            encode_parity_game(&manager, &game, rng, radix, num_groups, compute_strategy).unwrap();

        let alpha = if rng.random_bool(0.5) {
            Player::Even
        } else {
            Player::Odd
        };
        let vplayer = symbolic.players(&all_vertices).unwrap();

        // Seed the attractor with an arbitrary priority block rather than all of one player's
        // vertices, so most runs actually grow the set over several iterations instead of
        // converging in one step.
        let (_priority, u) = match symbolic.max_priority(&all_vertices).unwrap() {
            Some(result) => result,
            None => return, // an empty game has nothing to attract towards
        };

        let progress = silent_attractor_progress();
        let (incremental, incremental_strategy) = symbolic
            .attractor(alpha, &u, &all_vertices, &vplayer, None, None, &progress)
            .unwrap();
        let (naive, naive_strategy) = symbolic
            .attractor_naive(alpha, &u, &all_vertices, &vplayer, None, None, &progress)
            .unwrap();

        // `LDDFunction` has no `Debug` impl, so this can't be an `assert_eq!`.
        assert!(
            incremental == naive,
            "attractor and attractor_naive must compute the same attractor set"
        );
        assert_eq!(
            incremental_strategy.is_some(),
            naive_strategy.is_some(),
            "both must report a strategy exactly when compute_strategy is set"
        );

        if let Some(strategy) = &naive_strategy {
            symbolic
                .apply_strategy(alpha, strategy)
                .expect("attractor_naive's strategy must be a valid input to apply_strategy");
        }
    });
}
