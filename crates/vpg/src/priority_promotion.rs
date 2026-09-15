//! Authors: Maurice Laveaux
//!
//! Implementation of the priority promotion algorithm introduced in:
//!
//! > Massimo Benerecetti, Daniele Dell'Erba, and Fabio Mogavero. Solving
//! > Parity Games via Priority Promotion, pages 270-290. Springer International
//! > Publishing, Cham, 2016.
//!
//! Given the current triple (region_function, strategy, prio), referred to as
//! state, a region R is extracted by means of the query function. An alpha-region
//! is a set of vertices R and a witness strategy sigma. So that for all plays
//! consistent with sigma, they either stay within R and are winning for alpha.
//! Or they escape R via vertices having the highest priority in the game. The
//! initial state is (priority_function, empty_strategy, min(range(priority_function)))
//!
//! If the region R is a dominion in the whole game, the dominion is removed from
//! the game and the algorithm runs on the remaining game. If the extracted region
//! is open in the subgame G >= prio. Which is the subgame with only vertices
//! having greater or equal priority then prio. The next state becomes
//! (region_function\[R -> prio\], strategy\*, min(range(region_function >= prio))).
//!
//! If the alpha-region is a dominion in the subgame G >= prio, the lowest priority
//! region that the opponent can flee to is determined in [`PriorityPromotionSolver::promote_sub_dominion`].
//! And then the next state becomes (region_function\*\[R -> prio\*\], strategy\*, prio\*), and
//! in region_function\* all regions below prio\* are reset to the original priority.
//!
//! The strategy\* was not presented in the paper, but was partially determined
//! by the follow up paper Improving Priority Promotion for parity games. For
//! the attractor for some region R the strategy is determined by the witness
//! that alpha can reach R. For vertices inside the region R a witness core strategy
//! sigma is given. For all vertices inside R where no strategy is defined yet
//! an arbitrary successor inside R is taken to complete the strategy.

use std::collections::VecDeque;

use bitvec::bitvec;
use bitvec::order::Lsb0;
use log::debug;
use log::trace;

use crate::PG;
use crate::Player;
use crate::Pred;
use crate::Predecessors;
use crate::Priority;
use crate::Strat;
use crate::Strategy;
use crate::VertexIndex;
use crate::zielonka::Set;

/// Solves the given parity game using the priority promotion algorithm.
///
/// Returns the winning sets for both players and optionally their strategies.
pub fn solve_priority_promotion<G: PG>(game: &G, compute_strategy: bool) -> ([Set; 2], Option<[Strategy; 2]>) {
    if compute_strategy {
        let (solution, strategy) = solve_priority_promotion_impl::<G, Strategy>(game);
        (solution, Some(strategy))
    } else {
        let (solution, _) = solve_priority_promotion_impl::<G, ()>(game);
        (solution, None)
    }
}

/// Solves the given parity game using the priority promotion algorithm,
/// computing a strategy representation of type `S`.
fn solve_priority_promotion_impl<G: PG, S: Strat>(game: &G) -> ([Set; 2], [S; 2]) {
    debug_assert!(
        game.is_total(),
        "Priority promotion solver requires a total parity game"
    );

    let mut solver = PriorityPromotionSolver::new(game);
    let strategy = solver.solve::<S>();

    let mut w0 = bitvec![usize, Lsb0; 0; game.num_of_vertices()];
    let mut w1 = bitvec![usize, Lsb0; 0; game.num_of_vertices()];
    let mut s0 = S::new();
    let mut s1 = S::new();

    for v in game.iter_vertices() {
        let prio = solver.region_function[*v];
        debug_assert!(prio.is_none(), "All vertices should be solved");

        let winner = solver.final_winner[*v];
        match winner {
            Player::Even => {
                w0.set(*v, true);
                if game.owner(v) == Player::Even
                    && let Some(target) = strategy.get(v)
                {
                    s0.set(v, target);
                }
            }
            Player::Odd => {
                w1.set(*v, true);
                if game.owner(v) == Player::Odd
                    && let Some(target) = strategy.get(v)
                {
                    s1.set(v, target);
                }
            }
        }
    }

    ([w0, w1], [s0, s1])
}

/// Internal solver state for the priority promotion algorithm.
struct PriorityPromotionSolver<'a, G: PG> {
    game: &'a G,

    /// Precomputed predecessors for backward iteration.
    predecessors: Predecessors<'a>,

    /// Maps each vertex to its current region priority.
    /// `None` indicates a solved vertex.
    region_function: Vec<Option<Priority>>,

    /// Stores a list of vertices not yet solved by the algorithm.
    unsolved: Vec<VertexIndex>,

    /// Count the number of vertices per region, to speed up [`Self::next_priority`].
    regions: Vec<usize>,

    /// This is a reused queue with vertices to compute the attractor set from.
    todo: VecDeque<VertexIndex>,

    /// Reused per-vertex counter, used during attractor computation to count the
    /// number of successors of an opponent vertex that can still enter the
    /// region being computed.
    attractor_counters: Vec<usize>,

    /// The number of promotions required.
    promotions: usize,

    /// The number of dominions found.
    dominions: usize,

    /// Records the winning player for each vertex (set when a dominion is found).
    final_winner: Vec<Player>,
}

impl<'a, G: PG> PriorityPromotionSolver<'a, G> {
    /// Creates a new priority promotion solver for the given parity game.
    fn new(game: &'a G) -> Self {
        let num_vertices = game.num_of_vertices();

        // The lowest priority in the game (the highest number).
        let mut lowest_region = Priority::new(0);

        // Set region_function to the original priorities and initialize the mapping.
        let mut region_function = vec![None; num_vertices];
        let mut unsolved = Vec::with_capacity(num_vertices);

        for v in game.iter_vertices() {
            let prio = game.priority(v);
            region_function[*v] = Some(prio);
            unsolved.push(v);
            lowest_region = lowest_region.max(prio);
        }

        // Initialize all regions that have some vertices.
        let mut regions = vec![0usize; lowest_region.value() + 1];
        for &r in &region_function {
            if let Some(prio) = r {
                regions[prio.value()] += 1;
            }
        }

        PriorityPromotionSolver {
            game,
            predecessors: Predecessors::new(game),
            region_function,
            unsolved,
            regions,
            todo: VecDeque::new(),
            attractor_counters: vec![0; num_vertices],
            promotions: 0,
            dominions: 0,
            final_winner: vec![Player::Even; num_vertices],
        }
    }

    /// Compute winning strategies by means of priority promotion, follows the
    /// paper as closely as possible.
    ///
    /// # Details
    ///
    /// Important note: instead of actually repeatedly removing dominions from
    /// the game, the game is kept the same but the region_function is used to
    /// determine which vertices still are not solved. This is done because
    /// removing subgames allocates new memory repeatedly and parity games
    /// can be huge.
    fn solve<S: Strat>(&mut self) -> S {
        let mut strategy = S::new();

        // Find the highest priority in the game.
        let mut prio = self.next_priority(Priority::new(self.regions.len() - 1));

        // The algorithm was tail recursive so can also be written as iteration.
        loop {
            self.query(&mut strategy, prio);

            if self.is_open(prio, true) {
                debug!("Newly computed region is open in the subgame, with p = {}", prio);
                self.print_region(prio);

                // Keep the new region_function and substrategy, but go to the next priority.
                prio = self.next_priority(Priority::new(prio.value().saturating_sub(1)));
            } else if !self.is_open(prio, false) {
                // This is a dominion D in the whole game, compute the attractor
                // for this region.
                if self.solve_dominion(&mut strategy, prio) {
                    break; // Stop the algorithm, as all the vertices were solved.
                }

                // Reset the game and find the highest priority in the game.
                prio = self.next_priority(Priority::new(self.regions.len() - 1));
            } else {
                // The game is a dominion, but only in the subgame, so promote its priority.
                debug!("Promoted dominion D, with p = {}", prio);
                prio = self.promote_sub_dominion(&mut strategy, prio);
                debug!(" to {}", prio);
                self.print_region(prio);
            }
        }

        debug!(
            "{} dominions found, and {} promotions required",
            self.dominions, self.promotions
        );

        strategy
    }

    /// The current priority region is a dominion `D` in the whole game: attract
    /// it, record the winners, and remove it from the game, keeping the unsolved
    /// vertices and resetting lower priorities. Returns `true` when all vertices
    /// were solved, signalling the caller to stop.
    fn solve_dominion<S: Strat>(&mut self, strategy: &mut S, prio: Priority) -> bool {
        debug_assert!(self.todo.is_empty());

        for &v in &self.unsolved {
            if self.region_function[*v] == Some(prio) {
                self.todo.push_back(v);
            }
        }

        self.compute_attractor(strategy, prio, false);

        // Remove the dominion from the game and keep the unsolved vertices, also reset
        // lower priorities and set region of prio to the COMPUTED_REGION.
        debug!("Found the dominion D, with p = {}", prio);
        self.print_region(prio);

        // Record the winner for this dominion.
        let winner = Player::from_priority(prio);
        for v in self.game.iter_vertices() {
            if self.region_function[*v] == Some(prio) {
                self.final_winner[*v] = winner;
            }
        }

        // Reset the unsolved set and remove all regions, also add one dominion to statistics.
        self.unsolved.clear();
        self.regions.fill(0);
        self.dominions += 1;

        for v in self.game.iter_vertices() {
            if self.region_function[*v] == Some(prio) {
                // Assign a special region indicating that it's solved.
                self.region_function[*v] = None;
            } else if self.region_function[*v].is_some() {
                let original_prio = self.game.priority(v);
                self.region_function[*v] = Some(original_prio);
                strategy.remove(v);

                // Add the not solved vertices to the unsolved set and add vertices to their region.
                self.unsolved.push(v);
                self.regions[original_prio.value()] += 1;
            }
        }

        self.unsolved.is_empty()
    }

    /// From the state (region_function, strategy, prio) compute the new alpha-region
    /// R and update region_function\[R -> p\]. The strategy will be updated in
    /// [`Self::compute_attractor`]. The unsolved set is used to quickly iterate unsolved vertices.
    /// The todo queue is passed to be reused by [`Self::compute_attractor`].
    fn query<S: Strat>(&mut self, strategy: &mut S, prio: Priority) {
        // Make sure nothing else is stored in the todo.
        debug_assert!(self.todo.is_empty());

        // R* = region_function^-1(prio), this results in the todo for the attractor
        // computation, the initial set essentially.
        for &v in &self.unsolved {
            if self.region_function[*v] == Some(prio) {
                self.todo.push_back(v);
            }
        }

        // (region_function[R -> prio], strategy*) <- computeAttractor_G(todo, strategy
        // restricted to todo)
        self.compute_attractor(strategy, prio, true);
    }

    /// Compute the attractor set A for vertices in todo, with alpha being prio mod 2.
    /// `in_subgraph` indicates that only vertices in game <= prio are considered. This
    /// updates region_function\[A -> prio\]. The strategy is changed for alpha for the
    /// attraction witness. The remaining vertices of alpha without a strategy can pick
    /// any vertex inside A as witness.
    fn compute_attractor<S: Strat>(&mut self, strategy: &mut S, prio: Priority, in_subgraph: bool) {
        let alpha = Player::from_priority(prio);

        self.initialize_attractor_counters(alpha, prio, in_subgraph);

        // O(V + E): Compute the attractor set to the alpha-region.
        while let Some(w) = self.todo.pop_front() {
            // Check all predecessors v of w.
            for v in self.predecessors.predecessors(w) {
                // Skip predecessors that are already in the attractor set, also skip
                // vertices outside the subgame G <= prio. Or vertices that are computed.
                if self.region_function[*v] == Some(prio)
                    || self.region_function[*v].is_none()
                    || (in_subgraph && self.region_function[*v].is_some_and(|region| region > prio))
                {
                    continue;
                }

                if self.game.owner(v) == alpha {
                    // sigma(v) = w, a valid strategy for alpha is to pick a successor in A.
                    strategy.set(v, w);
                } else {
                    // One more successor of v (namely w) entered the region. The
                    // opponent vertex v is only attracted once every successor
                    // that could enter the region has actually done so.
                    self.attractor_counters[*v] -= 1;
                    if self.attractor_counters[*v] != 0 {
                        continue; // not in the attractor set yet!
                    }

                    // For opponent controlled vertices no strategy exists, so
                    // every possible outgoing edge is losing.
                }

                // Add a vertex to their new region and remove from the old one.
                let old_region = self.region_function[*v].expect("Unsolved vertices must have a region");
                self.regions[old_region.value()] -= 1;
                self.regions[prio.value()] += 1;

                // When this part is reached, all liberties of v are gone or v belongs
                // to alpha, so add vertex v to the attractor set.
                self.region_function[*v] = Some(prio);
                self.todo.push_back(v);
            }
        }

        self.assign_missing_strategies(strategy, alpha, prio);
    }

    /// Initialise, for every opponent vertex still under consideration, the
    /// number of its outgoing edges that lead to a vertex which can still
    /// enter the region with priority `prio` (a "poppable" target: not yet
    /// solved, and inside the subgame when `in_subgraph`). The opponent
    /// vertex is attracted once all of those targets have been attracted.
    fn initialize_attractor_counters(&mut self, alpha: Player, prio: Priority, in_subgraph: bool) {
        for i in 0..self.unsolved.len() {
            let v = self.unsolved[i];
            if self.game.owner(v) != alpha {
                self.attractor_counters[*v] = self
                    .game
                    .outgoing_edges(v)
                    .filter(|edge| {
                        let x = edge.to();
                        self.region_function[*x].is_some()
                            && !(in_subgraph && self.region_function[*x].is_some_and(|region| region > prio))
                    })
                    .count();
            }
        }
    }

    /// R \ domain(tau restricted to R*), essentially vertices in R belonging to
    /// alpha where no strategy is defined yet. These can pick an arbitrary
    /// successor that can reach R \ R*, these already have an attraction
    /// strategy so that is always fine.
    fn assign_missing_strategies<S: Strat>(&mut self, strategy: &mut S, alpha: Player, prio: Priority) {
        for &v in &self.unsolved {
            if self.region_function[*v] == Some(prio) && self.game.owner(v) == alpha && strategy.get(v).is_none() {
                for edge in self.game.outgoing_edges(v) {
                    let w = edge.to();

                    if self.region_function[*w] == Some(prio) {
                        // There exists some (v, w) in E such that w belongs to R (has r[w] == prio).
                        strategy.set(v, w);
                        break;
                    }
                }
            }
        }
    }

    /// Determine whether the alpha-region with priority `prio` is open in G, or
    /// in G <= prio (indicated by `in_subgraph`). This means that for all vertices
    /// v with region_function\[v\] equal to prio, this is set R. When v belongs
    /// to alpha, determined by prio mod 2, there is some witness successor in R.
    /// For opponent vertices all successors lead to R, no witness to escape basically.
    fn is_open(&self, prio: Priority, in_subgraph: bool) -> bool {
        let alpha = Player::from_priority(prio);

        // O(V): Loop over unsolved vertices and find vertices belonging to region with prio.
        for &v in &self.unsolved {
            if self.region_function[*v] == Some(prio) {
                if self.game.owner(v) != alpha {
                    // For all (v, u) in E, u should belong to R.
                    for edge in self.game.outgoing_edges(v) {
                        let u = edge.to();

                        // There is an edge from opponent to a vertex in the subgraph or in the whole graph.
                        if self.region_function[*u].is_some()
                            && ((in_subgraph && self.region_function[*u].is_some_and(|region| region < prio))
                                || (!in_subgraph && self.region_function[*u] != Some(prio)))
                        {
                            return true;
                        }
                    }
                } else {
                    // If there exists a (v, u) to R its closed.
                    let is_open = self
                        .game
                        .outgoing_edges(v)
                        .all(|edge| self.region_function[*edge.to()] != Some(prio));

                    if is_open {
                        return true;
                    }
                }
            }
        }

        false
    }

    /// Promote a sub dominion D to the minimum region greater than prio that the
    /// opponent can reach. This updates region_function\[D -> prio\*\] and resets
    /// all priorities greater than prio\* to the original priority function. The
    /// strategy is updated by means of [`Self::compute_attractor`]. And lower strategies
    /// are set to `None` (no strategy known).
    ///
    /// # Details
    ///
    /// This is referred to as r\* = bep(R, r) in the paper (best escape priority).
    /// For every opponent vertex this is the highest priority that it can reach.
    /// The region it can reach belongs to alpha,
    /// otherwise it would be attracted in some earlier state.
    fn promote_sub_dominion<S: Strat>(&mut self, strategy: &mut S, prio: Priority) -> Priority {
        let alpha = Player::from_priority(prio);

        // O(V): It is only a dominion in the subgraph, determine lowest p > prio
        // that opponent can escape to.
        let mut promotion = Priority::new(self.regions.len() - 1);

        for &v in &self.unsolved {
            if self.region_function[*v] == Some(prio) && self.game.owner(v) != alpha {
                // For all (v, u) in E collect the lowest priority larger than prio that opponent can flee to.
                for edge in self.game.outgoing_edges(v) {
                    let u = edge.to();

                    if let Some(region) = self.region_function[*u]
                        && region > prio
                    {
                        promotion = promotion.min(region);
                    }
                }
            }
        }

        self.apply_priority_promotion(strategy, prio, promotion);

        promotion
    }

    /// Promote the current region to `promotion`, reset all lower regions to
    /// their original priority, and clear their strategies.
    fn apply_priority_promotion<S: Strat>(&mut self, strategy: &mut S, prio: Priority, promotion: Priority) {
        self.promotions += 1;

        // Here the prio region is promoted to the new priority and all lower positions
        // are reset.
        for &v in &self.unsolved {
            if self.region_function[*v] == Some(prio) {
                // Promote the current region to the promotion priority.
                self.regions[prio.value()] -= 1;
                self.region_function[*v] = Some(promotion);
                self.regions[promotion.value()] += 1;
            } else if self.region_function[*v].is_some_and(|region| region < promotion) {
                // Reset all vertices higher to the original priorities, remove the strategy.
                let old_region = self.region_function[*v].expect("Unsolved vertices must have a region");
                self.regions[old_region.value()] -= 1;
                let original_prio = self.game.priority(v);
                self.region_function[*v] = Some(original_prio);
                strategy.remove(v);
                self.regions[original_prio.value()] += 1;
            }
        }
    }

    /// Print the vertices with region_function\[v\] equal to prio, representing the region.
    ///
    /// This costs O(V) so only enable this in debug.
    fn print_region(&self, prio: Priority) {
        if log::log_enabled!(log::Level::Trace) {
            let vertices: Vec<_> = self
                .unsolved
                .iter()
                .filter(|&&v| self.region_function[*v] == Some(prio))
                .map(|v| v.value())
                .collect();
            trace!(
                "alpha-region[{}] = {{ {} }}",
                prio,
                vertices.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
            );
        }
    }

    /// Computes max(rng(region_function <= prio)), so the next lower priority,
    /// smaller or equal to prio, that some vertex has.
    ///
    /// Starting from the current priority, find the next region that exists.
    /// This should never go out of bounds as the lowest region will always be a dominion.
    fn next_priority(&self, prio: Priority) -> Priority {
        let mut p = prio.value();
        while self.regions[p] == 0 {
            assert!(p > 0, "next_priority went out of bounds");
            p -= 1;
        }
        Priority::new(p)
    }
}

#[cfg(test)]
mod tests {
    use merc_io::DumpFiles;
    use merc_utilities::random_test;

    use crate::random_parity_game;
    use crate::solve_zielonka;
    use crate::verify_solution;
    use crate::write_pg;

    use super::*;

    #[test]
    #[cfg_attr(miri, ignore)] // Miri is too slow for this test.
    fn test_random_priority_promotion_solver() {
        random_test(100, |rng| {
            let files = DumpFiles::new("test_random_priority_promotion_solver");
            let game = random_parity_game(rng, true, 100, 5, 3);

            files.dump("input.pg", |writer| write_pg(writer, &game)).unwrap();

            let (solution, strategy) = solve_priority_promotion(&game, true);
            verify_solution(&game, &solution, &strategy.unwrap());
        });
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn test_priority_promotion_matches_zielonka() {
        random_test(100, |rng| {
            let game = random_parity_game(rng, true, 50, 4, 3);

            let (pp_solution, _) = solve_priority_promotion(&game, false);
            let (zielonka_solution, _) = solve_zielonka(&game, false);

            assert_eq!(
                pp_solution[0], zielonka_solution[0],
                "Even winning sets differ between priority promotion and Zielonka"
            );
            assert_eq!(
                pp_solution[1], zielonka_solution[1],
                "Odd winning sets differ between priority promotion and Zielonka"
            );
        });
    }
}
