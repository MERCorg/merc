use std::fmt;

use mcrl2_sys::cxx::CxxVector;
use mcrl2_sys::cxx::UniquePtr;
use mcrl2_sys::pbes::ffi::local_control_flow_graph;
use mcrl2_sys::pbes::ffi::local_control_flow_graph_vertex;
use mcrl2_sys::pbes::ffi::mcrl2_local_control_flow_graph_vertex_name;
use mcrl2_sys::pbes::ffi::mcrl2_local_control_flow_graph_vertex_value;
use mcrl2_sys::pbes::ffi::mcrl2_local_control_flow_graph_vertices;
use mcrl2_sys::pbes::ffi::mcrl2_stategraph_local_algorithm_cfgs;
use mcrl2_sys::pbes::ffi::mcrl2_stategraph_local_algorithm_run;
use mcrl2_sys::pbes::ffi::mcrl2_to_string;
use mcrl2_sys::pbes::ffi::mcrl2_propositional_variable_parameters;
use mcrl2_sys::pbes::ffi::mcrl2_propositional_variable_to_string;
use mcrl2_sys::pbes::ffi::mcrl2_srf_pbes_equation_variable;
use mcrl2_sys::pbes::ffi::mcrl2_srf_pbes_equations;
use mcrl2_sys::pbes::ffi::mcrl2_srf_pbes_to_pbes;
use mcrl2_sys::pbes::ffi::mcrl2_unify_parameters;
use mcrl2_sys::pbes::ffi::mcrl2_local_control_flow_graph_vertex_index;
use mcrl2_sys::pbes::ffi::pbes;
use mcrl2_sys::pbes::ffi::srf_equation;
use mcrl2_sys::pbes::ffi::srf_pbes;
use mcrl2_sys::pbes::ffi::stategraph_algorithm;
use merc_utilities::MercError;

use crate::Aterm;
use crate::AtermString;
use crate::AtermList;
use crate::DataExpression;
use crate::DataVariable;

/// mcrl2::pbes_system::pbes
pub struct Pbes {
    pbes: UniquePtr<pbes>,
}

impl Pbes {
    /// Load a PBES from a file.
    pub fn from_file(filename: &str) -> Result<Self, MercError> {
        Ok(Pbes {
            pbes: mcrl2_sys::pbes::ffi::mcrl2_load_pbes_from_pbes_file(filename)?,
        })
    }

    /// Load a PBES from a textual pbes file.
    pub fn from_text_file(filename: &str) -> Result<Self, MercError> {
        Ok(Pbes {
            pbes: mcrl2_sys::pbes::ffi::mcrl2_load_pbes_from_text_file(filename)?,
        })
    }

    /// Load a PBES from text.
    pub fn from_text(input: &str) -> Result<Self, MercError> {
        Ok(Pbes {
            pbes: mcrl2_sys::pbes::ffi::mcrl2_load_pbes_from_text(input)?,
        })
    }

    pub(crate) fn new(pbes: UniquePtr<pbes>) -> Self {
        Pbes { pbes }
    }
}

impl fmt::Display for Pbes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", mcrl2_to_string(&self.pbes))
    }
}

pub struct PbesStategraph {
    algorithm: UniquePtr<stategraph_algorithm>,
    control_flow_graphs: Vec<PbesStategraphControlFlowGraph>,
    control_flow_graphs_ffi: UniquePtr<CxxVector<local_control_flow_graph>>,
}

impl PbesStategraph {
    /// Run the state graph algorithm on the given PBES.
    pub fn run(pbes: &Pbes) -> Result<Self, MercError> {
        let algorithm = mcrl2_stategraph_local_algorithm_run(&pbes.pbes)?;

        // Obtain a copy of the control flow graphs.
        let mut control_flow_graphs_ffi = CxxVector::new();
        mcrl2_stategraph_local_algorithm_cfgs(control_flow_graphs_ffi.pin_mut(), &algorithm);

        Ok(PbesStategraph {
            algorithm,
            control_flow_graphs: control_flow_graphs_ffi
                .iter()
                .map(|cfg| PbesStategraphControlFlowGraph::new(cfg))
                .collect(),
            control_flow_graphs_ffi
        })
    }

    /// Returns the control flow graphs identified by the algorithm.
    pub fn control_flow_graphs(&self) -> &Vec<PbesStategraphControlFlowGraph> {
        &self.control_flow_graphs
    }
}

/// mcrl2::pbes_system::detail::local_control_flow_graph
pub struct PbesStategraphControlFlowGraph {
    cfg: *const local_control_flow_graph,
    vertices: Vec<ControlFlowGraphVertex>,
    vertices_ffi: UniquePtr<CxxVector<local_control_flow_graph_vertex>>,
}

impl PbesStategraphControlFlowGraph {

    /// Returns the vertices of the control flow graph.
    pub fn vertices(&self) -> &Vec<ControlFlowGraphVertex> {
        &self.vertices
    }

    pub(crate) fn new(cfg: *const local_control_flow_graph) -> Self {

        // Obtain the vertices of the control flow graph.
        let mut vertices_ffi = CxxVector::new();
        mcrl2_local_control_flow_graph_vertices(vertices_ffi.pin_mut(), unsafe { &*cfg });
        let vertices = vertices_ffi
            .iter()
            .map(|v| ControlFlowGraphVertex::new(v))
            .collect();

        PbesStategraphControlFlowGraph { cfg, vertices, vertices_ffi }
    }
}

/// mcrl2::pbes_system::detail::control_flow_graph_vertex
pub struct ControlFlowGraphVertex {
    vertex: *const local_control_flow_graph_vertex,
}

impl ControlFlowGraphVertex {
    /// Returns the name of the variable associated with this vertex.
    pub fn name(&self) -> AtermString {
        AtermString::new(Aterm::new(unsafe { mcrl2_local_control_flow_graph_vertex_name(self.vertex) }))
    }

    pub fn value(&self) -> DataExpression {
        DataExpression::new(Aterm::new(unsafe { mcrl2_local_control_flow_graph_vertex_value(self.vertex) }))
    }

    /// Returns the index of the variable associated with this vertex.
    pub fn index(&self) -> usize {
        unsafe { mcrl2_local_control_flow_graph_vertex_index(self.vertex) }
    }

    pub(crate) fn new(vertex: *const local_control_flow_graph_vertex) -> Self {
        ControlFlowGraphVertex { vertex }
    }
}

/// mcrl2::pbes_system::srf_pbes
pub struct SrfPbes {
    srf_pbes: UniquePtr<srf_pbes>,
    equations: Vec<SrfEquation>,
    ffi_equations: UniquePtr<CxxVector<srf_equation>>,
}

impl SrfPbes {
    /// Convert a PBES to an SRF PBES.
    pub fn from(pbes: &Pbes) -> Result<Self, MercError> {
        let srf_pbes = mcrl2_sys::pbes::ffi::mcrl2_to_srf_pbes(&pbes.pbes)?;

        let mut ffi_equations = CxxVector::new();
        mcrl2_srf_pbes_equations(ffi_equations.pin_mut(), &srf_pbes);

        Ok(SrfPbes {
            srf_pbes,
            equations: ffi_equations.iter().map(|eq| SrfEquation::new(eq)).collect(),
            ffi_equations,
        })
    }

    /// Convert the SRF PBES back to a PBES.
    pub fn to_pbes(&self) -> Pbes {
        Pbes::new(mcrl2_srf_pbes_to_pbes(self.srf_pbes.as_ref().unwrap()))
    }

    /// Unify all parameters of the equations.
    pub fn unify_parameters(&mut self, ignore_ce_equations: bool, reset: bool) -> Result<(), MercError> {
        mcrl2_unify_parameters(self.srf_pbes.pin_mut(), ignore_ce_equations, reset);
        Ok(())
    }

    /// Returns the srf equations of the SRF pbes.
    pub fn equations(&self) -> &Vec<SrfEquation> {
        &self.equations
    }
}

/// mcrl2::pbes_system::srf_equation
pub struct SrfEquation {
    equation: *const srf_equation,
}

impl SrfEquation {
    /// Returns the parameters of the equation.
    pub fn variable(&self) -> PropositionalVariable {
        PropositionalVariable::new(Aterm::new(unsafe { mcrl2_srf_pbes_equation_variable(self.equation) }))
    }

    /// Creates a new `SrfEquation` from the given FFI equation pointer.
    pub(crate) fn new(equation: *const srf_equation) -> Self {
        SrfEquation { equation }
    }
}

/// mcrl2::pbes_system::propositional_variable
pub struct PropositionalVariable {
    term: Aterm,
}

impl PropositionalVariable {
    /// Returns the parameters of the propositional variable.
    pub fn parameters(&self) -> AtermList<DataVariable> {
        let term = mcrl2_propositional_variable_parameters(self.term.get());
        AtermList::new(Aterm::new(term))
    }

    /// Creates a new `PbesPropositionalVariable` from the given term.
    pub(crate) fn new(term: Aterm) -> Self {
        PropositionalVariable { term }
    }
}

impl fmt::Debug for PropositionalVariable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", mcrl2_propositional_variable_to_string(self.term.get()))
    }
}
