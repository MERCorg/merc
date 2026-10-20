use log::debug;
use oxidd::BooleanFunction;
use oxidd::Function;
use oxidd::bdd::BDDFunction;
use oxidd::util::OptBool;
use oxidd::util::OutOfMemory;

use merc_lts::LTS;
use merc_lts::LabelledTransitionSystem;
use merc_lts::LtsBuilderMem;
use merc_lts::TransitionLabel;
use merc_symbolic::CubeIterAll;
use merc_symbolic::FormatConfigSet;
use merc_symbolic::bits_to_bdd;
use merc_utilities::MercError;
use merc_utilities::Timing;

use crate::FeatureDiagram;
use crate::FeatureTransitionSystem;
use crate::FeaturedLabel;

/// Projects all configurations of a featured transition system into standard labelled transition systems.
pub fn project_feature_transition_system_iter<'a, L: TransitionLabel>(
    fts: &'a FeatureTransitionSystem<L>,
    fd: &'a FeatureDiagram,
    timing: &'a Timing,
) -> impl Iterator<Item = Result<(ProjectedLts<FeaturedLabel<L>>, &'a Timing), MercError>> {
    let variables = fd
        .feature_names()
        .iter()
        .map(|name| {
            fd.features()
                .get(name)
                .expect("Feature diagram should contain all features defined in the feature names")
                .clone()
        })
        .collect::<Vec<_>>();

    CubeIterAll::new(fd.configuration()).map(move |cube| {
        let cube = cube?;

        let bdd = match bits_to_bdd(&fd.configuration().manager_ref(), &variables, &cube) {
            Ok(bdd) => bdd,
            Err(e) => return Err(MercError::from(e)),
        };

        let lts = timing.measure("project", || -> Result<_, MercError> {
            project_feature_transition_system(fts, &bdd)
        })?;

        Ok((ProjectedLts { bits: cube, bdd, lts }, timing))
    })
}

/// A projected configuration of a featured transition system.
pub struct ProjectedLts<L: TransitionLabel> {
    pub bits: Vec<OptBool>,
    pub bdd: BDDFunction,
    pub lts: LabelledTransitionSystem<L>,
}

/// Projects a featured transition system onto a specific feature selection, resulting in a standard labelled transition system.
pub fn project_feature_transition_system<L: TransitionLabel>(
    fts: &FeatureTransitionSystem<L>,
    feature_selection: &BDDFunction,
) -> Result<LabelledTransitionSystem<FeaturedLabel<L>>, MercError> {
    let mut builder = LtsBuilderMem::new(fts.labels().to_vec(), Vec::new());

    debug!("Projecting on feature selection {}", FormatConfigSet(feature_selection));

    let builder_labels = builder.label_indices(fts.labels());
    let labels = fts
        .labels()
        .iter()
        .map(|label| -> Result<bool, OutOfMemory> {
            // Check if the edge is enabled by the feature selection, if so, include it.
            let result = feature_selection.and(label.feature_expr())?.satisfiable();
            debug!("Label {} is included: {}", label, result);

            Ok(result)
        })
        .collect::<Result<Vec<_>, _>>()?;

    for v in fts.iter_states() {
        for edge in fts.outgoing_transitions(v) {
            if labels[edge.label] {
                builder.add_transition_index(v, builder_labels[edge.label.value()], edge.to);
            }
        }
    }

    Ok(builder.finish(fts.initial_state_index(), false))
}
