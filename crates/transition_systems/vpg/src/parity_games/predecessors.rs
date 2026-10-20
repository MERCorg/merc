//! Authors: Maurice Laveaux and Sjef van Loo

use std::marker::PhantomData;

use merc_collections::ByteCompressedVec;
use merc_collections::bytevec;

use crate::PG;
use crate::VertexIndex;

/// A trait for providing the predecessors of a parity game.
pub trait Pred {
    /// Returns an iterator over the predecessors the given vertex.
    fn predecessors(&self, vertex_index: VertexIndex) -> impl Iterator<Item = VertexIndex> + '_;
}

impl<T: Pred + ?Sized> Pred for &T {
    fn predecessors(&self, vertex_index: VertexIndex) -> impl Iterator<Item = VertexIndex> + '_ {
        (*self).predecessors(vertex_index)
    }
}

/// Stores the predecessors for a given parity game, implements [Pred].
pub struct Predecessors<'a> {
    /// A flat list of all predecessors in the game.
    edges_from: ByteCompressedVec<VertexIndex>,

    /// A mapping from the vertex to the `edges_from` that stores its
    /// predecessors.
    vertex_to_predecessors: ByteCompressedVec<usize>,

    /// Marker to tie the lifetime of the predecessors to the game.
    _marker: PhantomData<&'a ()>,
}

impl<'a> Predecessors<'a> {
    /// Creates the predecessors structure for the given parity game.
    pub fn new<G: PG>(game: &'a G) -> Self {
        let mut edges_from = bytevec![VertexIndex::new(0); game.num_of_edges()];
        let mut vertex_to_predecessors = bytevec![0; game.num_of_vertices()];

        // Count the number of incoming transitions for each vertex
        for vertex_index in game.iter_vertices() {
            for edge in game.outgoing_edges(vertex_index) {
                vertex_to_predecessors.update(edge.to().value(), |start| *start += 1);
            }
        }

        // Compute the start offsets (prefix sum)
        vertex_to_predecessors.fold(0, |offset, start| {
            let new_offset = offset + *start;
            *start = offset;
            new_offset
        });

        // Place the transitions
        for vertex_index in game.iter_vertices() {
            for edge in game.outgoing_edges(vertex_index) {
                vertex_to_predecessors.update(edge.to().value(), |start| {
                    edges_from.set(*start, vertex_index);
                    *start += 1;
                });
            }
        }

        let mut offset = 0usize;
        vertex_to_predecessors.fold(0, |previous, start| {
            let result = *start;
            *start = previous;
            offset = result;
            result
        });

        vertex_to_predecessors.push(offset);

        Self {
            edges_from,
            vertex_to_predecessors,
            _marker: PhantomData,
        }
    }
}

impl Pred for Predecessors<'_> {
    fn predecessors(&self, vertex_index: VertexIndex) -> impl Iterator<Item = VertexIndex> + '_ {
        let start = self.vertex_to_predecessors.index(vertex_index.value());
        let end = self.vertex_to_predecessors.index(vertex_index.value() + 1);
        (start..end).map(move |i| self.edges_from.index(i))
    }
}

#[cfg(test)]
mod tests {
    use bitvec::bitvec;
    use bitvec::order::Lsb0;
    use itertools::Itertools;
    use merc_utilities::random_test;

    use crate::PG;
    use crate::Pred;
    use crate::Predecessors;
    use crate::PrioSubgame;
    use crate::Priority;
    use crate::Subgame;
    use crate::random_parity_game;

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_predecessors() {
        random_test(100, |rng| {
            let game = random_parity_game(rng, true, 50, 10, 5);
            check_predecessors(game);
        })
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_predecessors_subgame() {
        random_test(100, |rng| {
            let pg = random_parity_game(rng, true, 50, 10, 5);
            let game = PrioSubgame::new(&pg, Priority::new(0));

            check_predecessors(game);
        })
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_random_pred_trait_subgame() {
        random_test(100, |rng| {
            let pg = random_parity_game(rng, true, 50, 10, 5);

            let mut restricted = bitvec![usize, Lsb0; 0; pg.num_of_vertices()];
            for vertex in pg.iter_vertices() {
                if pg.priority(vertex) <= Priority::new(2) {
                    restricted.set(vertex.value(), true);
                }
            }

            let predecessors = Predecessors::new(&pg);
            let subgame = Subgame::new(&pg, restricted, predecessors);

            check_pred_trait(&subgame);
        })
    }

    /// Checks that the predecessors computed by the `Predecessors` structure
    /// match the expected predecessors computed by iterating over all vertices
    /// and their outgoing edges.
    fn check_predecessors<G: PG>(game: G) {
        let predecessors = Predecessors::new(&game);
        for vertex in game.iter_vertices() {
            // Compute the expected predecessors by iterating over all vertices and their outgoing edges.
            let expected_predecessors: Vec<_> = game
                .iter_vertices()
                .filter(|&v| game.outgoing_edges(v).any(|e| e.to() == vertex))
                .collect();
            let actual_predecessors: Vec<_> = predecessors.predecessors(vertex).collect();

            assert_eq!(
                expected_predecessors.into_iter().sorted().collect::<Vec<_>>(),
                actual_predecessors.into_iter().sorted().collect::<Vec<_>>(),
                "Predecessors of vertex {} do not match",
                vertex.value()
            );
        }
    }

    /// Checks that the `Pred` trait implementation for a game matches
    /// predecessors computed from outgoing edges.
    fn check_pred_trait<G: PG + Pred>(game: &G) {
        for vertex in game.iter_vertices() {
            let expected_predecessors: Vec<_> = game
                .iter_vertices()
                .filter(|&v| game.outgoing_edges(v).any(|e| e.to() == vertex))
                .collect();
            let actual_predecessors: Vec<_> = game.predecessors(vertex).collect();

            assert_eq!(
                expected_predecessors.into_iter().sorted().collect::<Vec<_>>(),
                actual_predecessors.into_iter().sorted().collect::<Vec<_>>(),
                "Predecessors of vertex {} do not match",
                vertex.value()
            );
        }
    }
}
