use oxidd::ldd::LDDFunction;

use merc_utilities::MercError;

use crate::ExtendedParityGame;
use crate::Player;
use crate::solve_symbolic_zielonka;
use crate::symbolic::SymbolicParityGame;
use crate::symbolic::SymbolicSolution;
use crate::symbolic::symbolic_zielonka::includes;

#[cfg(test)]
use crate::PG;
#[cfg(test)]
use crate::ParityGameBuilder;
#[cfg(test)]
use crate::solve_zielonka;
#[cfg(test)]
use crate::symbolic::convert_symbolic_parity_game;
#[cfg(test)]
use crate::verify_solution;
#[cfg(test)]
use oxidd::ManagerRef;
#[cfg(test)]
use oxidd::ldd::LDDManagerRef;

/// Checks that `solution.strategy` really does win the vertices `solution.winning` claims.
///
/// # Details
///
/// This is the efficient certificate: it stays entirely at the LDD level, so it scales to large
/// models the same way the rest of the symbolic solver does. Restrict `game` to the initial
/// vertex's winner's own strategy via [`SymbolicParityGame::apply_strategy`], re-solve the
/// restricted game from scratch, and require the two solutions to agree.
///
/// Returns an error describing the mismatch if the strategy fails to certify.
pub fn verify_symbolic_strategy(
    game: &SymbolicParityGame,
    initial_vertex: &LDDFunction,
    solution: &SymbolicSolution,
) -> Result<(), MercError> {
    let winner = solution
        .winner(initial_vertex)
        .ok_or("verify_symbolic_strategy: initial vertex was not resolved by the solver")?;
    let strategy = solution.strategy[winner.to_index()]
        .as_ref()
        .ok_or("verify_symbolic_strategy: solution has no strategy (game was not built with compute_strategy)")?;

    let restricted = game.apply_strategy(winner, strategy)?;
    let new_sinks = restricted.sinks(restricted.vertices(), restricted.vertices())?;

    let epg = ExtendedParityGame::new(restricted, initial_vertex.clone(), new_sinks);
    let (new_winner, new_solution) = solve_symbolic_zielonka(&epg, false)?;

    if new_winner != winner {
        return Err(format!(
            "verify_symbolic_strategy: restricting {winner}'s own strategy changed the winner of the initial \
             vertex to {new_winner}"
        )
        .into());
    }

    let solved = solution.winning[0].union(&solution.winning[1])?;
    if includes(&solved, game.vertices())? {
        for player in [Player::Even, Player::Odd] {
            let i = player.to_index();
            if solution.winning[i] != new_solution.winning[i] {
                return Err(format!(
                    "verify_symbolic_strategy: after restricting {winner}'s strategy, {player}'s winning set \
                     changed even though the original solution covered every vertex"
                )
                .into());
            }
        }
    } else {
        for player in [Player::Even, Player::Odd] {
            let i = player.to_index();
            if !includes(&new_solution.winning[i], &solution.winning[i])? {
                return Err(format!(
                    "verify_symbolic_strategy: after restricting {winner}'s strategy, {player}'s winning set shrank"
                )
                .into());
            }
        }
    }

    Ok(())
}

/// Independently certifies a [`SymbolicSolution`] against the game it was computed for, by
/// decoding to an explicit [`crate::ParityGame`] and re-solving with [`solve_zielonka`].
///
/// A slow, differently-implemented cross-check for tests only — production code uses the
/// efficient [`verify_symbolic_strategy`] instead, which never leaves the LDD level.
///
/// `solution` must have been computed over `game.vertices()` (in practice, the same game passed
/// to [`crate::solve_symbolic_zielonka`]). Panics if the certified strategy is inconsistent, or
/// if the two solvers disagree on any vertex.
#[cfg(test)]
pub(crate) fn verify_symbolic_solution(
    manager: &LDDManagerRef,
    game: &SymbolicParityGame,
    solution: &SymbolicSolution,
) -> Result<(), MercError> {
    let (decoded, cubes) = convert_symbolic_parity_game(manager, game, game.vertices())?;
    let total = make_parity_game_total(&decoded);

    let (certified, strategy) = solve_zielonka(&total, true);
    verify_solution(&total, &certified, &strategy.expect("compute_strategy was true"));

    for (index, cube) in cubes.iter().enumerate() {
        let singleton = manager.with_manager_shared(|m| LDDFunction::singleton(m, cube))?;
        let symbolic_winner = solution
            .winner(&singleton)
            .ok_or("verify_symbolic_solution: vertex was not resolved by the symbolic solver")?;
        let certified_winner = if certified[Player::Even.to_index()][index] {
            Player::Even
        } else {
            Player::Odd
        };

        if symbolic_winner != certified_winner {
            panic!(
                "verify_symbolic_solution: vertex {index} (cube {cube:?}) is won by {symbolic_winner} \
                symbolically but by {certified_winner} in the independently certified solution"
            );
        }
    }

    Ok(())
}

/// Totalizes `game` using [`ParityGameBuilder::finish`]'s own `make_total`: every vertex without
/// an outgoing edge gets a self-loop whose priority is [`Player::opponent`] of its owner, so the
/// self-loop's unique play is won by the *opponent* of the sink's owner. That is the same
/// convention [`SymbolicParityGame::compute_total_graph`] uses for its sinks (a disjunctive PBES
/// equation with no enabled summand is `false`) — this function exists only so both paths share
/// that one implementation, since [`convert_symbolic_parity_game`] deliberately keeps sinks
/// rather than fabricating self-loops itself.
///
/// Shared by [`verify_symbolic_solution`] and `symbolic_zielonka`'s random-test oracle.
#[cfg(test)]
pub(crate) fn make_parity_game_total<G: PG>(game: &G) -> crate::ParityGame {
    let mut builder = ParityGameBuilder::with_capacity(game.initial_vertex(), game.num_of_edges());
    for vertex in game.iter_vertices() {
        builder.add_vertex(vertex, game.owner(vertex), game.priority(vertex));
    }
    for vertex in game.iter_vertices() {
        for edge in game.outgoing_edges(vertex) {
            builder.add_edge(vertex, edge.to());
        }
    }
    builder.finish(true, true)
}

#[cfg(test)]
mod tests {
    use rand::RngExt;

    use oxidd::ManagerRef;
    use oxidd::ldd::LDDFunction;

    use merc_symbolic::LDD_CACHE_CAPACITY;
    use merc_symbolic::LDD_NODE_CAPACITY;
    use merc_utilities::random_test;

    use crate::ExtendedParityGame;
    use crate::encode_parity_game;
    use crate::random_parity_game;
    use crate::solve_symbolic_zielonka;

    use super::verify_symbolic_solution;
    use super::verify_symbolic_strategy;

    /// Cross-checks [`verify_symbolic_solution`] against random *total* games: `symbolic_zielonka`'s
    /// own random test already cross-checks the solution against the explicit solver, so this only
    /// needs to show the certificate itself is accepted.
    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_verify_symbolic_solution_random_total() {
        random_test(100, |rng| {
            let game = random_parity_game(rng, true, 60, 5, 3);

            let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
            let radix = rng.random_range(2..=5);
            let num_groups = rng.random_range(1..=3);
            let (symbolic, all_vertices, cubes) =
                encode_parity_game(&manager, &game, rng, radix, num_groups, false).unwrap();

            let empty_sinks = symbolic.sinks(&all_vertices, &all_vertices).unwrap();
            let initial = manager
                .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[0]))
                .unwrap();

            // Force a full solve (no early termination) so every vertex is covered.
            let epg = ExtendedParityGame::new(symbolic, initial, empty_sinks);
            let (_, solution) = solve_symbolic_zielonka(&epg, false).unwrap();

            verify_symbolic_solution(&manager, &epg.game, &solution).unwrap();
        });
    }

    /// Same as [`test_verify_symbolic_solution_random_total`], but for random *non-total* games,
    /// so the deadlock/sink handling is exercised too.
    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_verify_symbolic_solution_random_non_total() {
        random_test(100, |rng| {
            let game = random_parity_game(rng, false, 60, 5, 3);

            let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
            let radix = rng.random_range(2..=5);
            let num_groups = rng.random_range(1..=3);
            let (symbolic, all_vertices, cubes) =
                encode_parity_game(&manager, &game, rng, radix, num_groups, false).unwrap();

            let sinks = symbolic.sinks(&all_vertices, &all_vertices).unwrap();
            let initial = manager
                .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[0]))
                .unwrap();

            let epg = ExtendedParityGame::new(symbolic, initial, sinks);
            let (_, solution) = solve_symbolic_zielonka(&epg, false).unwrap();

            verify_symbolic_solution(&manager, &epg.game, &solution).unwrap();
        });
    }

    /// Cross-checks [`verify_symbolic_strategy`] (native, LDD-level certification via
    /// [`crate::SymbolicParityGame::apply_strategy`]) against random *total* games.
    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_verify_symbolic_strategy_random_total() {
        random_test(100, |rng| {
            let game = random_parity_game(rng, true, 60, 5, 3);

            let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
            let radix = rng.random_range(2..=5);
            let num_groups = rng.random_range(1..=3);
            let (symbolic, all_vertices, cubes) =
                encode_parity_game(&manager, &game, rng, radix, num_groups, true).unwrap();

            let empty_sinks = symbolic.sinks(&all_vertices, &all_vertices).unwrap();
            let initial = manager
                .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[0]))
                .unwrap();

            let epg = ExtendedParityGame::new(symbolic, initial, empty_sinks);
            let (_, solution) = solve_symbolic_zielonka(&epg, false).unwrap();

            verify_symbolic_strategy(&epg.game, &epg.initial_vertex, &solution).unwrap();
        });
    }

    /// Same as [`test_verify_symbolic_strategy_random_total`], but for random *non-total* games,
    /// so `apply_strategy`'s fresh-sink handling is exercised too.
    #[test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_verify_symbolic_strategy_random_non_total() {
        random_test(100, |rng| {
            let game = random_parity_game(rng, false, 60, 5, 3);

            let manager = oxidd::ldd::new_manager(LDD_NODE_CAPACITY, LDD_CACHE_CAPACITY, 1);
            let radix = rng.random_range(2..=5);
            let num_groups = rng.random_range(1..=3);
            let (symbolic, all_vertices, cubes) =
                encode_parity_game(&manager, &game, rng, radix, num_groups, true).unwrap();

            let sinks = symbolic.sinks(&all_vertices, &all_vertices).unwrap();
            let initial = manager
                .with_manager_shared(|m| LDDFunction::singleton(m, &cubes[0]))
                .unwrap();

            let epg = ExtendedParityGame::new(symbolic, initial, sinks);
            let (_, solution) = solve_symbolic_zielonka(&epg, false).unwrap();

            verify_symbolic_strategy(&epg.game, &epg.initial_vertex, &solution).unwrap();
        });
    }
}
