use log::debug;
use merc_lts::TransitionLabel;
use oxidd::BooleanFunction;
use oxidd::ManagerRef;
use oxidd::bdd::BDDFunction;
use oxidd::bdd::BDDManagerRef;

use merc_collections::ByteCompressedVec;
use merc_lts::LTS;
use merc_syntax::Action;
use merc_syntax::MultiAction;
use merc_syntax::StateFrm;
use merc_utilities::MercError;

use crate::FeatureTransitionSystem;
use crate::ModalEquationSystem;
use crate::Translation;
use crate::VariabilityParityGame;
use crate::VertexIndex;
use crate::compute_reachable;
use crate::make_vpg_total;
use crate::warn_unknown_action_labels;

/// Translates a feature transition system into a variability parity game.
pub fn translate_vpg(
    manager_ref: &BDDManagerRef,
    fts: &FeatureTransitionSystem<String>,
    configuration: BDDFunction,
    formula: &StateFrm,
) -> Result<VariabilityParityGame, MercError> {
    // Parses all labels into MultiAction once
    let parsed_labels: Result<Vec<MultiAction>, MercError> = fts
        .labels()
        .iter()
        .map(|label| {
            if label.is_tau_label() {
                Ok(MultiAction::tau())
            } else {
                MultiAction::parse(label.label())
            }
        })
        .collect();

    // Simplify the labels by stripping BDD information
    let simplified_labels: Vec<MultiAction> = parsed_labels?
        .iter()
        .map(strip_feature_configuration_from_multi_action)
        .collect();

    // Warn about any labels that are used in the formula but do not correspond to any label in the LTS.
    warn_unknown_action_labels(formula, &simplified_labels);

    let true_bdd = manager_ref.with_manager_shared(|manager| BDDFunction::t(manager));

    let equation_system = ModalEquationSystem::new(formula);
    debug!("{}", equation_system);
    let mut algorithm: Translation<'_, _, BDDFunction> = Translation::new(fts, &simplified_labels, &equation_system);

    algorithm.translate(
        fts.initial_state_index(),
        0,
        |transition| match transition {
            Some(transition) => fts.labels()[transition.label].feature_expr().clone(),
            None => true_bdd.clone(), // The label does not matter, allow all configurations.
        },
        |existing, new| {
            *existing = existing.or(&new)?;
            Ok(())
        },
    )?;

    // Convert the feature diagram (with names) to a VPG
    let variables: Vec<BDDFunction> = fts.features().values().cloned().collect();

    let vertices = algorithm.vertices();
    let result = VariabilityParityGame::from_edges(
        manager_ref,
        VertexIndex::new(0),
        vertices.iter().map(|vertex| vertex.0).collect(),
        ByteCompressedVec::with_iter(vertices.iter().map(|vertex| vertex.1)),
        configuration,
        variables,
        || algorithm.edges().iter().cloned(),
    );

    // Check that all vertices are reachable from the initial vertex. After
    // totality it could be that the true or false nodes are not reachable.
    if cfg!(debug_assertions) {
        let (_, reachable_vertices) = compute_reachable(&result);
        debug_assert!(
            reachable_vertices.iter().all(|v| v.is_some()),
            "Not all vertices are reachable from the initial vertex"
        );
    }

    // Ensure that the result is a total VPG.
    let total_result = if !result.is_vpg_total(manager_ref)? {
        make_vpg_total(manager_ref, &result)?
    } else {
        result
    };

    Ok(total_result)
}

/// Removes the BDD information from the multi-action, i.e., only keeps the action labels.
fn strip_feature_configuration_from_multi_action(multi_action: &MultiAction) -> MultiAction {
    MultiAction {
        actions: multi_action
            .actions
            .iter()
            .map(|action| Action {
                id: action.id.clone(),
                args: Vec::new(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use merc_macros::merc_test;
    use merc_symbolic::BDD_CACHE_CAPACITY;
    use merc_symbolic::BDD_NODE_CAPACITY;
    use merc_syntax::UntypedStateFrmSpec;

    use crate::FeatureDiagram;
    use crate::compute_reachable;
    use crate::read_fts;
    use crate::translate_vpg;

    #[merc_test]
    #[cfg_attr(miri, ignore)] // Oxidd does not work with miri
    fn test_vpg_running_example() {
        let manager_ref = oxidd::bdd::new_manager(BDD_NODE_CAPACITY, BDD_CACHE_CAPACITY, 1);

        let fd = FeatureDiagram::from_reader(
            &manager_ref,
            include_bytes!("../../../examples/vpg/running_example_fts.fd") as &[u8],
        )
        .unwrap();
        let fts = read_fts(
            &manager_ref,
            include_bytes!("../../../examples/vpg/running_example_fts.aut") as &[u8],
            fd.features().clone(),
        )
        .unwrap();

        let formula = UntypedStateFrmSpec::parse(include_str!("../../../examples/vpg/running_example.mcf")).unwrap();

        let vpg = translate_vpg(&manager_ref, &fts, fd.configuration().clone(), &formula.formula).unwrap();

        assert!(
            vpg.is_vpg_total(&manager_ref).unwrap(),
            "The translated VPG should be total"
        );
        assert!(
            compute_reachable(&vpg).1.iter().all(|v| v.is_some()),
            "All vertices should be reachable from the initial vertex"
        );
    }
}
