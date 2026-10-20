use oxidd::BooleanFunction;
use oxidd::Function;
use oxidd::bdd::BDDFunction;
use oxidd::util::OptBool;

use merc_symbolic::CubeIterAll;
use merc_symbolic::bits_to_bdd;
use merc_utilities::MercError;
use merc_utilities::Timing;

use crate::PG;
use crate::ParityGame;
use crate::VariabilityParityGame;

/// Projects all configurations of a variability parity game into standard parity games.
pub fn project_variability_parity_games_iter<'a>(
    vpg: &'a VariabilityParityGame,
    timing: &'a Timing,
) -> impl Iterator<Item = Result<(Projected, &'a Timing), MercError>> {
    CubeIterAll::new(vpg.configuration()).map(move |cube| {
        let cube = cube?;
        let bdd = match bits_to_bdd(&vpg.configuration().manager_ref(), vpg.variables(), &cube) {
            Ok(bdd) => bdd,
            Err(e) => return Err(MercError::from(e)),
        };

        let pg = timing.measure("project", || -> Result<_, MercError> {
            project_variability_parity_game(vpg, &bdd)
        })?;

        Ok((
            Projected {
                bits: cube,
                bdd,
                game: pg,
            },
            timing,
        ))
    })
}

/// A projected configuration of a variability parity game.
pub struct Projected {
    pub bits: Vec<OptBool>,
    pub bdd: BDDFunction,
    pub game: ParityGame,
}

/// Projects a variability parity game into a standard parity game by removing
/// edges that are not enabled by the given feature selection.
pub fn project_variability_parity_game(
    vpg: &VariabilityParityGame,
    feature_selection: &BDDFunction,
) -> Result<ParityGame, MercError> {
    let mut edges = Vec::new();

    for v in vpg.iter_vertices() {
        for edge in vpg.outgoing_edges(v) {
            // Check if the edge is enabled by the feature selection, if so, include it.
            if feature_selection.and(edge.label())?.satisfiable() {
                edges.push((v, edge.to()));
            }
        }
    }

    Ok(ParityGame::from_edges(
        vpg.initial_vertex(),
        vpg.owners().clone(),
        vpg.priorities().clone(),
        true, // It can be that after removing edges the result is not a total parity game.
        || edges.iter().cloned(),
    ))
}
