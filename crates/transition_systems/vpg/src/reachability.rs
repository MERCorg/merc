//! Authors: Maurice Laveaux and Sjef van Loo

use std::collections::VecDeque;

use bitvec::bitvec;
use bitvec::order::Lsb0;

use merc_collections::ByteCompressedVec;
use merc_collections::CompressedEntry;

use crate::PG;
use crate::ParityGame;
use crate::PlayerVec;
use crate::VertexIndex;

/// Computes the reachable portion of a parity game from the initial vertex.
///
/// Returns a new parity game containing only reachable vertices and a mapping
/// from old vertex indices to new vertex indices (None for unreachable vertices).
pub fn compute_reachable<G: PG>(game: &G) -> (ParityGame, Vec<Option<usize>>) {
    let num_vertices = game.num_of_vertices();

    // Mapping from old vertex indices to new vertices (None means unreachable)
    let mut mapping = vec![None; num_vertices];
    let mut visited = bitvec![usize, Lsb0; 0; num_vertices];

    // New game data structures. The reachable part is at most as large as the
    // original game and its priorities are a subset of the original priorities,
    // so sizing both up front means neither has to grow or widen its entries.
    let mut new_owners = PlayerVec::with_capacity(num_vertices);
    let mut new_priorities =
        ByteCompressedVec::with_capacity(num_vertices, game.highest_priority().value().bytes_required());
    let mut new_vertices = vec![0]; // Start with offset 0
    // New vertex indices are always < num_vertices, so the required width is
    // known up front.
    let mut new_edges_to: ByteCompressedVec<VertexIndex> =
        ByteCompressedVec::with_capacity(game.num_of_edges(), num_vertices.bytes_required());

    // Helper closure to add a vertex to the new game
    let mut add_vertex = |v: VertexIndex| -> usize {
        if let Some(idx) = mapping[*v] {
            return idx;
        }

        // Add a new vertex
        let new_v = new_owners.len();
        new_owners.push(game.owner(v));
        new_priorities.push(game.priority(v));

        // Update mapping
        mapping[*v] = Some(new_v);
        new_v
    };

    // BFS from initial vertex
    let mut queue = VecDeque::new();
    let initial = game.initial_vertex();
    queue.push_back(initial);
    visited.set(*initial, true);

    while let Some(v) = queue.pop_front() {
        // Ensure the current vertex exists in the new game
        let new_v = add_vertex(v);
        debug_assert_eq!(new_v, new_vertices.len() - 1);

        // Process all outgoing edges
        for edge in game.outgoing_edges(v) {
            let new_w = add_vertex(edge.to());
            new_edges_to.push(VertexIndex::new(new_w));

            if !visited[*edge.to()] {
                visited.set(*edge.to(), true);
                queue.push_back(edge.to());
            }
        }

        // Update vertex offset for next vertex
        new_vertices.push(new_edges_to.len());
    }

    // Find new initial vertex
    let new_initial_idx = mapping[*initial].expect("Initial vertex is unreachable, which should be impossible");
    let new_initial = VertexIndex::new(new_initial_idx);

    let new_game = ParityGame::new(new_initial, new_owners, new_priorities, new_vertices, new_edges_to);

    (new_game, mapping)
}
