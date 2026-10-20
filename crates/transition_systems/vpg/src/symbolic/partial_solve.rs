use std::ops::ControlFlow;

use oxidd::ldd::LDDFunction;

use merc_symbolic::merge;
use merc_utilities::MercError;

use crate::Player;
use crate::symbolic::AttractorProgress;
use crate::symbolic::RecursionProgress;
use crate::symbolic::SymbolicParityGame;
use crate::symbolic::SymbolicSolution;
use crate::symbolic::symbolic_zielonka::ExtendedParityGame;
use crate::symbolic::symbolic_zielonka::empty_set;
use crate::symbolic::symbolic_zielonka::empty_strategy_pair;
use crate::symbolic::symbolic_zielonka::includes;
use crate::symbolic::symbolic_zielonka::total_graph_with_early_exit;
use crate::symbolic::symbolic_zielonka::zielonka;

/// Which of two equivalent ways [`detect_solitair_cycles`]/[`detect_forced_cycles`]/
/// [`detect_fatal_attractors`] (and their `_within_safe_vertices` counterparts) stay sound against
/// `incomplete`'s unlearned edges. Resolved once, right after `total` is known (via
/// [`Self::resolve`]), and threaded from there as one value.
///
/// [`Self::Restricted`] is not cheaper than [`Self::Safe`]: [`Self::resolve`] pays two extra full
/// `safe_vertices` attractor fixed points up front to build it. See the "On-the-fly and partial
/// solving" page on the merc-website developer docs for the derivation.
enum SafetyMode {
    /// Fold `incomplete` directly into every attractor/control-predecessor call.
    Safe,
    /// Restrict the whole search up front to [`SymbolicParityGame::safe_vertices`].
    Restricted([LDDFunction; 2]),
}

impl SafetyMode {
    /// Resolves the mode: `Restricted` (computing both players' `safe_vertices` against `total`)
    /// when `restrict_to_safe_vertices`, else `Safe`.
    fn resolve(
        restrict_to_safe_vertices: bool,
        game: &SymbolicParityGame,
        total: &LDDFunction,
        incomplete: &LDDFunction,
        attractor_progress: &AttractorProgress,
    ) -> Result<Self, MercError> {
        if restrict_to_safe_vertices {
            Ok(Self::Restricted([
                game.safe_vertices(Player::Even, total, incomplete, attractor_progress)?,
                game.safe_vertices(Player::Odd, total, incomplete, attractor_progress)?,
            ]))
        } else {
            Ok(Self::Safe)
        }
    }
}

/// Solves as much of `epg.game.vertices()` as [`SymbolicParityGame::compute_total_graph`] and two
/// plain `zielonka` calls, restricted to the *safe* vertices of each player, can resolve.
///
/// Extends `partial_solution` rather than replacing it, and returns early once
/// `epg.initial_vertex` is resolved; sound but not necessarily complete, so a real subset of
/// `epg.game.vertices()` going unresolved is expected, not a bug.
pub fn partial_solve(
    epg: &ExtendedParityGame,
    incomplete: &LDDFunction,
    partial_solution: SymbolicSolution,
    recursion_progress: &RecursionProgress,
    attractor_progress: &AttractorProgress,
) -> Result<SymbolicSolution, MercError> {
    let SymbolicSolution {
        mut winning,
        mut strategy,
    } = partial_solution;

    let total = match total_graph_with_early_exit(epg, incomplete, &mut winning, &mut strategy, attractor_progress)? {
        ControlFlow::Break(solution) => return Ok(solution),
        ControlFlow::Continue(total) => total,
    };
    let game = &epg.game;

    let safe_even = game.safe_vertices(Player::Even, &total, incomplete, attractor_progress)?;
    let mut solution0 = zielonka(game, &safe_even, recursion_progress, attractor_progress)?;
    solution0.winning[0] = solution0.winning[0].union(&winning[0])?;
    if game.compute_strategy() {
        let updated = solution0.strategy[0]
            .as_ref()
            .expect("compute_strategy is set")
            .union(strategy[0].as_ref().expect("compute_strategy is set"))?;
        solution0.strategy[0] = Some(updated);
    }

    if includes(&solution0.winning[0], &epg.initial_vertex)? {
        solution0.winning[1] = winning[1].clone();
        if game.compute_strategy() {
            solution0.strategy[1] = strategy[1].clone();
        }
        return Ok(solution0);
    }

    let safe_odd = game.safe_vertices(Player::Odd, &total, incomplete, attractor_progress)?;
    let mut solution1 = zielonka(game, &safe_odd, recursion_progress, attractor_progress)?;
    solution1.winning[1] = solution1.winning[1].union(&winning[1])?;
    if game.compute_strategy() {
        let updated = solution1.strategy[1]
            .as_ref()
            .expect("compute_strategy is set")
            .union(strategy[1].as_ref().expect("compute_strategy is set"))?;
        solution1.strategy[1] = Some(updated);
    }

    solution1.winning[0] = solution0.winning[0].clone();
    if game.compute_strategy() {
        solution1.strategy[0] = solution0.strategy[0].clone();
    }

    Ok(solution1)
}

/// Detects "solitair" winning cycles: a set `U` of vertices all owned by `alpha`, each with at
/// least one edge staying inside `U`, so `alpha` can simply choose to loop inside `U` forever
/// (winning it outright, since `U` only ever contains vertices at `alpha`'s own parity).
///
/// Folds `incomplete` directly into every attractor call — see
/// [`detect_solitair_cycles_within_safe_vertices`] for the variant that instead restricts the
/// whole search up front to the safe vertices of each player.
pub fn detect_solitair_cycles(
    epg: &ExtendedParityGame,
    incomplete: &LDDFunction,
    partial_solution: SymbolicSolution,
    attractor_progress: &AttractorProgress,
) -> Result<SymbolicSolution, MercError> {
    detect_solitair_cycles_impl(epg, incomplete, false, partial_solution, attractor_progress)
}

/// Like [`detect_solitair_cycles`], but restricts the whole search up front to
/// [`SymbolicParityGame::safe_vertices`] instead of folding `incomplete` directly into every
/// attractor call. This finds the same dominion, at a higher up-front cost (two extra full
/// `safe_vertices` attractor fixed points), rather than a different (cheaper or more
/// conservative) one — see the "On-the-fly and partial solving" page on the merc-website
/// developer docs for the derivation.
pub fn detect_solitair_cycles_within_safe_vertices(
    epg: &ExtendedParityGame,
    incomplete: &LDDFunction,
    partial_solution: SymbolicSolution,
    attractor_progress: &AttractorProgress,
) -> Result<SymbolicSolution, MercError> {
    detect_solitair_cycles_impl(epg, incomplete, true, partial_solution, attractor_progress)
}

fn detect_solitair_cycles_impl(
    epg: &ExtendedParityGame,
    incomplete: &LDDFunction,
    restrict_to_safe_vertices: bool,
    partial_solution: SymbolicSolution,
    attractor_progress: &AttractorProgress,
) -> Result<SymbolicSolution, MercError> {
    let SymbolicSolution {
        mut winning,
        mut strategy,
    } = partial_solution;

    let total = match total_graph_with_early_exit(epg, incomplete, &mut winning, &mut strategy, attractor_progress)? {
        ControlFlow::Break(solution) => return Ok(solution),
        ControlFlow::Continue(total) => total,
    };
    let game = &epg.game;

    let vplayer = game.players(&total)?;
    let parity = game.parity(&total)?;
    let mode = SafetyMode::resolve(restrict_to_safe_vertices, game, &total, incomplete, attractor_progress)?;

    for alpha in [Player::Even, Player::Odd] {
        let i = alpha.to_index();

        let mut u = empty_set(game)?;
        let mut u_next = parity[i].intersect(&vplayer[i])?;
        if let SafetyMode::Restricted(safe) = &mode {
            u_next = u_next.intersect(&safe[i])?;
        }

        while u != u_next {
            u = u_next.clone();
            u_next = game.predecessors(&u, &u)?;
        }

        accept_cycle(
            game,
            alpha,
            &u,
            &total,
            &vplayer,
            &mode,
            incomplete,
            &mut winning,
            &mut strategy,
            attractor_progress,
        )?;
    }

    Ok(SymbolicSolution { winning, strategy })
}

/// Detects "forced" winning cycles: a set `U` of vertices (any owner) such that `alpha` can
/// *force* play to stay inside `U` forever regardless of what its opponent does — unlike
/// [`detect_solitair_cycles`], `U` is not restricted to `alpha`'s own vertices, so an
/// opponent-owned vertex only joins `U` once every one of its edges is proven to stay inside.
///
/// Folds `incomplete` directly into every attractor/control-predecessor call — see
/// [`detect_forced_cycles_within_safe_vertices`] for the variant that instead restricts the whole
/// search up front to the safe vertices of each player.
pub fn detect_forced_cycles(
    epg: &ExtendedParityGame,
    incomplete: &LDDFunction,
    partial_solution: SymbolicSolution,
    attractor_progress: &AttractorProgress,
) -> Result<SymbolicSolution, MercError> {
    detect_forced_cycles_impl(epg, incomplete, false, partial_solution, attractor_progress)
}

/// Like [`detect_forced_cycles`], but restricts the whole search up front to
/// [`SymbolicParityGame::safe_vertices`] instead of folding `incomplete` directly into every
/// attractor/control-predecessor call. This finds the same dominion, at a higher up-front cost,
/// rather than a different (cheaper or more conservative) one.
pub fn detect_forced_cycles_within_safe_vertices(
    epg: &ExtendedParityGame,
    incomplete: &LDDFunction,
    partial_solution: SymbolicSolution,
    attractor_progress: &AttractorProgress,
) -> Result<SymbolicSolution, MercError> {
    detect_forced_cycles_impl(epg, incomplete, true, partial_solution, attractor_progress)
}

fn detect_forced_cycles_impl(
    epg: &ExtendedParityGame,
    incomplete: &LDDFunction,
    restrict_to_safe_vertices: bool,
    partial_solution: SymbolicSolution,
    attractor_progress: &AttractorProgress,
) -> Result<SymbolicSolution, MercError> {
    let SymbolicSolution {
        mut winning,
        mut strategy,
    } = partial_solution;

    let total = match total_graph_with_early_exit(epg, incomplete, &mut winning, &mut strategy, attractor_progress)? {
        ControlFlow::Break(solution) => return Ok(solution),
        ControlFlow::Continue(total) => total,
    };
    let game = &epg.game;

    let vplayer = game.players(&total)?;
    let parity = game.parity(&total)?;
    let mode = SafetyMode::resolve(restrict_to_safe_vertices, game, &total, incomplete, attractor_progress)?;

    for alpha in [Player::Even, Player::Odd] {
        let i = alpha.to_index();

        let mut u = empty_set(game)?;
        let mut u_next = parity[i].clone();
        if let SafetyMode::Restricted(safe) = &mode {
            u_next = u_next.intersect(&safe[i])?;
        }

        while u != u_next {
            u = u_next.clone();
            u_next = match &mode {
                SafetyMode::Safe => {
                    u.intersect(&game.control_predecessors_within(alpha, &u, &total, &vplayer, Some(incomplete))?)?
                }
                SafetyMode::Restricted(safe) => {
                    u.intersect(&game.control_predecessors_within(alpha, &u, &safe[i], &vplayer, None)?)?
                }
            };
        }

        accept_cycle(
            game,
            alpha,
            &u,
            &total,
            &vplayer,
            &mode,
            incomplete,
            &mut winning,
            &mut strategy,
            attractor_progress,
        )?;
    }

    Ok(SymbolicSolution { winning, strategy })
}

/// Records `U` as won, with an overapproximate `merge(U, U)` strategy and
/// extends `winning`/`strategy` with the attractor into `U`.
///
/// Shared by [`detect_solitair_cycles_impl`] and [`detect_forced_cycles_impl`],
/// which differ only in how they compute `u`, not in what happens once they
/// have it.
#[allow(clippy::too_many_arguments)]
fn accept_cycle(
    game: &SymbolicParityGame,
    alpha: Player,
    u: &LDDFunction,
    total: &LDDFunction,
    vplayer: &[LDDFunction; 2],
    mode: &SafetyMode,
    incomplete: &LDDFunction,
    winning: &mut [LDDFunction; 2],
    strategy: &mut [Option<LDDFunction>; 2],
    attractor_progress: &AttractorProgress,
) -> Result<(), MercError> {
    let i = alpha.to_index();

    if game.compute_strategy() {
        let extra = merge(game.manager(), u, u)?;
        let existing = strategy[i].take().expect("compute_strategy is set");
        strategy[i] = Some(existing.union(&extra)?);
    }

    let (attracted, attr_strategy) = match mode {
        SafetyMode::Safe => game.attractor(alpha, u, total, vplayer, Some(incomplete), None, attractor_progress)?,
        SafetyMode::Restricted(safe) => game.attractor(alpha, u, &safe[i], vplayer, None, None, attractor_progress)?,
    };

    winning[i] = winning[i].union(&attracted)?;
    if game.compute_strategy() {
        let existing = strategy[i].take().expect("compute_strategy is set");
        strategy[i] = Some(existing.union(&attr_strategy.expect("compute_strategy is set"))?);
    }

    Ok(())
}

/// Detects fatal attractors: for each priority `c`, searches for a set of
/// priority-`c` vertices that `alpha = Player::from_priority(c)` can force play
/// to always return to — winning `alpha` the whole attractor into that set.
///
/// This operation does not yield a strategy; it only updates the winning sets.
///
/// `w0`/`w1` seed the winning sets.
pub fn detect_fatal_attractors(
    epg: &ExtendedParityGame,
    incomplete: &LDDFunction,
    w0: &LDDFunction,
    w1: &LDDFunction,
    attractor_progress: &AttractorProgress,
) -> Result<[LDDFunction; 2], MercError> {
    detect_fatal_attractors_impl(epg, incomplete, false, w0, w1, attractor_progress)
}

/// Like [`detect_fatal_attractors`], but restricted to safe vertices.
pub fn detect_fatal_attractors_within_safe_vertices(
    epg: &ExtendedParityGame,
    incomplete: &LDDFunction,
    w0: &LDDFunction,
    w1: &LDDFunction,
    attractor_progress: &AttractorProgress,
) -> Result<[LDDFunction; 2], MercError> {
    detect_fatal_attractors_impl(epg, incomplete, true, w0, w1, attractor_progress)
}

#[allow(clippy::too_many_arguments)]
fn detect_fatal_attractors_impl(
    epg: &ExtendedParityGame,
    incomplete: &LDDFunction,
    restrict_to_safe_vertices: bool,
    w0: &LDDFunction,
    w1: &LDDFunction,
    attractor_progress: &AttractorProgress,
) -> Result<[LDDFunction; 2], MercError> {
    let mut winning = [w0.clone(), w1.clone()];
    let mut strategy = empty_strategy_pair(&epg.game)?;

    let total = match total_graph_with_early_exit(epg, incomplete, &mut winning, &mut strategy, attractor_progress)? {
        ControlFlow::Break(solution) => return Ok(solution.winning),
        ControlFlow::Continue(total) => total,
    };
    let game = &epg.game;

    let vplayer = game.players(&total)?;
    let mode = SafetyMode::resolve(restrict_to_safe_vertices, game, &total, incomplete, attractor_progress)?;

    // Ascending order: from least to most significant priority.
    for (&priority, block) in game.priorities() {
        let alpha = Player::from_priority(priority);
        let i = alpha.to_index();
        let search_space = match &mode {
            SafetyMode::Safe => &total,
            SafetyMode::Restricted(safe) => &safe[i],
        };

        // Restricting the seed to `search_space` up front saves a wasted
        // `monotone_attractor` fixpoint.
        let mut x = block.intersect(search_space)?;
        let mut y = empty_set(game)?;

        while !x.is_empty() && x != y {
            y = x.clone();
            let z = game.monotone_attractor(
                &x,
                alpha,
                priority,
                search_space,
                &vplayer,
                matches!(mode, SafetyMode::Safe).then_some(incomplete),
                None,
            )?;

            if includes(&z, &x)? {
                let (attracted, _) = match &mode {
                    SafetyMode::Safe => {
                        game.attractor(alpha, &z, &total, &vplayer, Some(incomplete), None, attractor_progress)?
                    }
                    SafetyMode::Restricted(_) => {
                        game.attractor(alpha, &z, search_space, &vplayer, None, None, attractor_progress)?
                    }
                };
                winning[i] = winning[i].union(&attracted)?;
                break;
            }
            x = x.intersect(&z)?;
        }
    }

    Ok(winning)
}
