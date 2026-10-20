use merc_utilities::MercError;
use oxidd::BooleanFunction;
use oxidd::ManagerRef;
use oxidd::bdd::BDDFunction;
use oxidd::bdd::BDDManagerRef;

use merc_symbolic::minus;

use crate::PG;
use crate::Player;
use crate::Priority;
use crate::VariabilityParityGame;
use crate::VariabilityParityGameBuilder;
use crate::VertexIndex;

/// Makes the given variability parity game total by adding edges to true/false nodes as needed.
pub fn make_vpg_total(
    manager_ref: &BDDManagerRef,
    vpg: &VariabilityParityGame,
) -> Result<VariabilityParityGame, MercError> {
    // The universe for totality is the game's overall configuration, not global true.
    let universe = manager_ref.with_manager_shared(|manager| BDDFunction::t(manager));

    let mut builder = VariabilityParityGameBuilder::with_capacity(
        vpg.initial_vertex(),
        vpg.num_of_edges() + vpg.num_of_vertices() + 2,
    );

    for vertex in vpg.iter_vertices() {
        builder.add_vertex(vertex, vpg.owner(vertex), vpg.priority(vertex));
    }

    // Owner does not matter, priority must be even for true node and odd for false node.
    let mut true_node = None;
    let mut false_node = None;

    for vertex in vpg.iter_vertices() {
        let mut all_outgoing = manager_ref.with_manager_shared(|manager| BDDFunction::f(manager));
        for edge in vpg.outgoing_edges(vertex) {
            // Add the original edge.
            builder.add_edge(vertex, edge.label().clone(), edge.to());

            // Keep track of the overall outgoing configuration.
            all_outgoing = all_outgoing.or(edge.label())?;
        }

        // Missing configurations are those in the universe not covered by any outgoing edge.
        let missing = minus(&universe, &all_outgoing)?;
        if missing.satisfiable() {
            if vpg.owner(vertex) == Player::Odd {
                // Odd player deadlock: add edge to true node for the remaining configurations.
                let node = if let Some(node) = true_node {
                    node
                } else {
                    let node = VertexIndex::new(builder.num_of_vertices());
                    builder.add_vertex(node, Player::Even, Priority::new(0));
                    true_node = Some(node);
                    node
                };
                builder.add_edge(vertex, universe.clone(), node);
            } else {
                // Even player deadlock: add edge to false node for the remaining configurations.
                let node = if let Some(node) = false_node {
                    node
                } else {
                    let node = VertexIndex::new(builder.num_of_vertices());
                    builder.add_vertex(node, Player::Even, Priority::new(1));
                    false_node = Some(node);
                    node
                };
                builder.add_edge(vertex, universe.clone(), node);
            }
        }
    }

    // Add self-loops for sink nodes if they were created
    if let Some(node) = true_node {
        builder.add_edge(node, universe.clone(), node);
    }
    if let Some(node) = false_node {
        builder.add_edge(node, universe.clone(), node);
    }

    Ok(builder.finish(manager_ref, vpg.configuration().clone(), vpg.variables().clone(), false))
}
