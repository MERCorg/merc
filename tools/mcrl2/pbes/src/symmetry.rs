#![allow(non_snake_case)]
/// Authors: Menno Bartels and Maurice Laveaux
/// To keep consistent with the theory we allow non-snake case names.

use std::iter;

use itertools::Itertools;

use log::info;
use mcrl2::DataVariable;
use mcrl2::Pbes;
use mcrl2::SrfPbes;
use mcrl2::PbesStategraph;
use mcrl2::PbesStategraphControlFlowGraph;
use merc_utilities::MercError;
use merc_io::TimeProgress;

use crate::permutation::Permutation;
use crate::permutation::permutation_group;

/// Implements symmetry detection for PBESs.
pub struct SymmetryAlgorithm {
    state_graph: PbesStategraph, // Needs to be kept alive while the control flow graphs are used.

    parameters: Vec<DataVariable>, // The parameters of the unified SRF PBES.

    all_control_flow_parameters: Vec<usize>, // Keeps track of all parameters identified as control flow parameters.
}

/// Returns the index of the variable that the control flow graph considers
fn variable_index(cfg: &PbesStategraphControlFlowGraph,) -> usize {
    // Check that all the vertices have the same variable assigned for consistency
    cfg.vertices().iter().for_each(|v| {
        if v.index() != cfg.vertices().first().expect("There is at least one vertex in a CFG").index() {
            panic!("Inconsistent variable indices in control flow graph.");
        }
    });


    for v in cfg.vertices() {
        // Simply return the index of the variable
        return v.index();
    }

    panic!("No variable found in control flow graph.");
}

impl SymmetryAlgorithm {
    /// Does the required preprocessing to analyse symmetries in the given PBES.
    pub fn new(pbes: &Pbes) -> Result<Self, MercError> {
        // Apply various preproecessing necessary for symmetry detection
        let mut srf = SrfPbes::from(pbes)?;
        srf.unify_parameters(false, false)?;

        info!("==== SRF PBES ====");
        info!("{}", srf.to_pbes());

        let parameters = if let Some(equation) = srf.equations().first() {
            equation.variable().parameters().to_vec()
        } else {
            // There are no equations, so no parameters.
            Vec::new()
        };

        info!("Unified parameters: {:?}", parameters);

        let state_graph = PbesStategraph::run(&srf.to_pbes());
        let all_control_flow_parameters = state_graph
            .control_flow_graphs()
            .iter()
            .map(|cfg| variable_index(cfg))
            .collect::<Vec<_>>();        

        Ok(Self {
            state_graph,
            all_control_flow_parameters,
            parameters,
        })
    }

    /// Runs the symmetry detection algorithm.
    pub fn run(&self) {
        let cliques = self.cliques();

        for clique in cliques {
            info!("Found clique: {:?}", clique);
        }

        let _progress = TimeProgress::new(|_: ()| {}, 1);


    }

    /// Determine the cliques in the given control flow graphs.
    fn cliques(&self) -> Vec<Vec<usize>> {
        let mut cal_I = Vec::new();

        for (i, cfg) in self.state_graph.control_flow_graphs().iter().enumerate() {
            if cal_I.iter().any(|clique: &Vec<usize>| clique.contains(&i)) {
                // Skip every graph that already belongs to a clique.
                continue;
            }

            // For every other control flow graph check if it is compatible, and start a new clique
            let mut clique = vec![i];
            for j in (i + 1)..self.state_graph.control_flow_graphs().len() {
                if self.compatible(cfg, &self.state_graph.control_flow_graphs()[j])? == () {
                    clique.push(j);
                }
            }

            if clique.len() > 1 {
                cal_I.push(clique);
            }
        }

        cal_I
    }

    /// Computes the set of candidates we can derive from a single clique
    fn clique_candidates(&self, I: &Vec<usize>) -> impl Iterator<Item = Permutation> {

        // Determine the parameter indices involved in the clique
        let parameter_indices: Vec<usize> = I.iter()
            .map(|&i| {
                let cfg = &self.state_graph.control_flow_graphs()[i];
                variable_index(cfg)
            })
            .collect();

        // Groups the parameters by their sort.
        let same_sort_parameters = {
            let mut result = Vec::new();
            for param in self.parameters {   
                let sort = param.sort();
                if let Some(group) = result.iter_mut().find(|g: &&mut Vec<_>| {
                    if let Some(first) = g.first() {
                        self.parameters[*first].sort() == sort
                    } else {
                        false
                    }
                }) {
                    group.push(param);
                } else {
                    result.push(vec![param]);
                }
            }
            result
        };

        let mut all_data_groups: Box<dyn Iterator<Item = Permutation>> = Box::new(iter::empty());
        for group in same_sort_parameters {
            info!("Group of same sort parameters: {:?}", group);

            let parameter_indices: Vec<usize> = group.iter()
                .map(|param| {
                    self.parameters.iter().position(|p| p.name() == param.name()).unwrap()
                })
                .collect();

            all_data_groups = Box::new(
                all_data_groups
                    .cartesian_product(permutation_group(&parameter_indices))
                    .map(|(a, b)| a.concat(&b))
            ) as Box<dyn Iterator<Item = Permutation>>;
        }

        permutation_group(&parameter_indices).cartesian_product(all_data_groups)
    }

    /// Returns true iff the two control flow graphs are compatible.
    fn compatible(&self, left: &PbesStategraphControlFlowGraph, right: &PbesStategraphControlFlowGraph) ->  Result<(), MercError> {
        // First check whether the vertex sets are compatible.
        if let Err(x) = self.vertex_sets_compatible(left, right) {
            return Err("Incompatible vertex sets.".into());
        }

        // Further checks can be added here.

        Ok(())
    }

    /// Checks whether two control flow graphs have compatible vertex sets, meaning that the PVI and values of the
    /// vertices match.
    fn vertex_sets_compatible(&self, c: &PbesStategraphControlFlowGraph, c_prime: &PbesStategraphControlFlowGraph) -> Result<(), MercError> {
        if c.vertices().len() != c_prime.vertices().len() {
            return Err(
                format!("Different number of vertices ({} vs {}).", c.vertices().len(), c_prime.vertices().len()).into()
            );
        }

        for vertex in c.vertices() {
            c_prime.vertices().iter().any(|vertex_prime| {
                vertex.name() == vertex_prime.name() && vertex.value() == vertex_prime.value()
            })
        }

        Ok(())
    }
}
