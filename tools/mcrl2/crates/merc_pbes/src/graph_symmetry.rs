use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::io::Write;
use std::path::PathBuf;

use itertools::Itertools;
use log::debug;
use log::info;
use log::trace;
use petgraph::graph::NodeIndex;
use petgraph::graph::UnGraph;
use petgraph::graph6::ToGraph6;

use mcrl2::ATermRef;
use mcrl2::DataAbstractionRef;
use mcrl2::DataApplicationRef;
use mcrl2::DataExpressionRef;
use mcrl2::DataFunctionSymbol;
use mcrl2::DataFunctionSymbolRef;
use mcrl2::DataMachineNumberRef;
use mcrl2::DataVariable;
use mcrl2::DataVariableRef;
use mcrl2::Pbes;
use mcrl2::PbesConnective;
use mcrl2::PbesExistsRef;
use mcrl2::PbesExpression;
use mcrl2::PbesExpressionRef;
use mcrl2::PbesFlattenIter;
use mcrl2::PbesFlattenStack;
use mcrl2::PbesForallRef;
use mcrl2::PbesImpRef;
use mcrl2::PbesNotRef;
use mcrl2::PbesPropositionalVariableInstantiationRef;
use mcrl2::SortExpression;
use mcrl2::flatten_associative;
use mcrl2::is_abstraction;
use mcrl2::is_application;
use mcrl2::is_function_symbol;
use mcrl2::is_machine_number;
use mcrl2::is_pbes_and;
use mcrl2::is_pbes_exists;
use mcrl2::is_pbes_forall;
use mcrl2::is_pbes_imp;
use mcrl2::is_pbes_not;
use mcrl2::is_pbes_or;
use mcrl2::is_pbes_propositional_variable_instantiation;
use mcrl2::is_untyped_identifier;
use mcrl2::is_variable;
use mcrl2::is_where_clause;
use merc_utilities::MercError;

use crate::debruijn::BindingScope;
use crate::explore_common::UNIFY_IGNORE_CE_EQUATIONS;
use crate::explore_common::UNIFY_RESET_PARAMETERS;
use crate::permutation::Permutation;

/// Binary function symbols treated as commutative; listing a non-commutative one unsoundly widens the symmetry group.
const COMMUTATIVE_FUNCTION_SYMBOLS: &[&str] = &["&&", "||", "==", "!=", "<=>", "+", "*", "max", "min"];

/// Subset of [`COMMUTATIVE_FUNCTION_SYMBOLS`] that are also associative, and as
/// such can be flattened.
const ASSOCIATIVE_FUNCTION_SYMBOLS: &[&str] = &["&&", "||", "+", "*", "max", "min"];

/// Subset of [`ASSOCIATIVE_FUNCTION_SYMBOLS`] that are also idempotent, so that
/// repeated operands of a flattened chain can be collapsed. For the others
/// (`+`, `*`) the multiplicity of an operand is semantically relevant.
const IDEMPOTENT_FUNCTION_SYMBOLS: &[&str] = &["&&", "||", "max", "min"];

/// True iff `name` is a known commutative binary symbol at the given arity.
fn is_commutative(name: &str, arity: usize) -> bool {
    arity == 2 && COMMUTATIVE_FUNCTION_SYMBOLS.contains(&name)
}

/// True iff chains of `name` are flattened into one n-ary vertex.
fn is_flat_operator(name: &str, arity: usize) -> bool {
    arity == 2 && ASSOCIATIVE_FUNCTION_SYMBOLS.contains(&name)
}

/// A vertex of the symmetry detection graph.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum SdgVertex {
    /// The k'th (0-based) data parameter `d_k`. Allocated first, so
    /// `NodeIndex(k) == Parameter(k)`.
    Parameter(usize),

    /// A subformula or subterm, identified up to alpha-equivalence (see
    /// [`VertexKey`]). The term is the first one found with this structure.
    Term(PbesExpression),

    /// The update position `X_{i,k}`, one per PVI vertex and parameter.
    Update { pvi: usize, parameter: usize },
}

/// `C(v)`, the colour of a vertex, excluding `C_RHS` (see [`Sdg::rhs`]).
///
/// The colour only describes the vertex locally; the structure below it is
/// given by the edges.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum VertexColour {
    /// `C(x) = par`, refined by the sort so that only parameters of the same
    /// sort can be interchanged.
    Parameter(SortExpression),

    /// A bound variable, coloured by its de Bruijn index (see
    /// [`crate::debruijn`]) instead of the paper's `C(x) = x`, so that
    /// alpha-equivalent terms share a vertex.
    BoundVariable { sort: SortExpression, index: usize },

    /// `C(f(t1,...,tk)) = f`, including the sort of `f` to distinguish
    /// overloads. Constants are leaves.
    Function(DataFunctionSymbol),

    /// An untyped identifier, which should not occur in a well-typed PBES.
    Untyped(String),

    /// A machine number, coloured by its value.
    MachineNumber(u64),

    /// `C(phi1 (+) phi2) = (+)`, extended from `{and, or}` with `not` and `imp`.
    Connective(Connective),

    /// `C(Qe:D.phi) = (Q,D)`, generalised to a vector of sorts.
    Quantifier(Quantifier, Vec<SortExpression>),

    /// `C(X(t1,...,tn)) = pvi`.
    Pvi(String),

    /// `C(X_{i,k}) = update`.
    Update,
}

impl fmt::Display for VertexColour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VertexColour::Parameter(s) => write!(f, "par:{s}"),
            VertexColour::BoundVariable { sort, index } => write!(f, "bvar:{sort}#{index}"),
            VertexColour::Function(symbol) => f.write_str(symbol.name()),
            VertexColour::Untyped(name) => f.write_str(name),
            VertexColour::MachineNumber(n) => write!(f, "{n}"),
            VertexColour::Connective(c) => write!(f, "{c}"),
            VertexColour::Quantifier(q, ss) => write!(f, "{q} {}", ss.iter().format(",")),
            VertexColour::Pvi(name) => write!(f, "{name}"),
            VertexColour::Update => f.write_str("update"),
        }
    }
}

impl VertexColour {
    fn dot_fill_colour(&self) -> &'static str {
        match self {
            VertexColour::Parameter(_) => "#aec6cf",
            VertexColour::BoundVariable { .. } => "#d5e8d4",
            VertexColour::Function(_) | VertexColour::Untyped(_) => "#fff2cc",
            VertexColour::MachineNumber(_) => "#ffe6cc",
            VertexColour::Connective(_) => "#f8cecc",
            VertexColour::Quantifier(..) => "#e1d5e7",
            VertexColour::Pvi(_) => "#dae8fc",
            VertexColour::Update => "#f5f5f5",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Connective {
    And,
    Or,
    Not,
    Imp,
}

impl fmt::Display for Connective {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Connective::And => "&&",
            Connective::Or => "||",
            Connective::Not => "!",
            Connective::Imp => "=>",
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Quantifier {
    Forall,
    Exists,
    /// Data-level lambda abstraction (`lambda x:D. body`).
    Lambda,
    /// Data-level set/bag comprehension.
    Comprehension,
}

impl fmt::Display for Quantifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Quantifier::Forall => "forall",
            Quantifier::Exists => "exists",
            Quantifier::Lambda => "lambda",
            Quantifier::Comprehension => "comp",
        })
    }
}

impl fmt::Display for EdgeColour {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EdgeColour::Uncoloured => Ok(()),
            EdgeColour::Argument(positions) => write!(f, "{}", positions.iter().format(",")),
            EdgeColour::Update(roles) => write!(f, "{}", roles.iter().format(",")),
            EdgeColour::Multiplicity(count) => write!(f, "x{count}"),
        }
    }
}

/// `C(e)`, the colour of an edge.
///
/// GAP does not support parallel edges, so coinciding edges are merged into one
/// edge whose colour is a set of positions or roles.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum EdgeColour {
    /// The positions `{i | t_i = psi}` of argument `psi` of a non-commutative
    /// function, e.g. `{1,3}` for `x` in `f(x,y,x)`.
    Argument(BTreeSet<usize>),

    /// An argument of a commutative function, or any other edge.
    Uncoloured,

    /// The roles of an update vertex's edge, e.g. `{data, par}` when parameter
    /// `d_k` is passed unchanged.
    Update(BTreeSet<UpdateRole>),

    /// How often an operand occurs in a flattened non-idempotent chain, so that
    /// `n + n + m` and `n + m + m` differ.
    Multiplicity(usize),
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
enum UpdateRole {
    Pvi,
    Data,
    Par,
}

impl fmt::Display for UpdateRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UpdateRole::Pvi => "pvi",
            UpdateRole::Data => "data",
            UpdateRole::Par => "par",
        })
    }
}

/// The symmetry detection graph (SDG) of a PBES, as constructed by
/// [`build_sdg`].
pub struct Sdg {
    /// A simple undirected graph: no parallel edges (see [`EdgeColour`]) and no
    /// self-loops.
    graph: UnGraph<SdgVertex, EdgeColour>,

    /// `C(v)`, indexed by `NodeIndex::index()`.
    colours: Vec<VertexColour>,

    /// `C_RHS(v) = {X | v = phi_X}`, indexed by `NodeIndex::index()`.
    rhs: Vec<BTreeSet<usize>>,

    /// The unified parameter vector; `parameters[k]` is the vertex
    /// `NodeIndex(k)`.
    parameters: Vec<DataVariable>,

    /// Names of the predicate variables in equation order, for diagnostics.
    equation_names: Vec<String>,
}

impl Sdg {
    /// Returns the number of parameters, the domain of the symmetries.
    pub fn num_parameters(&self) -> usize {
        self.parameters.len()
    }

    /// Returns the number of vertices in the graph.
    pub fn num_vertices(&self) -> usize {
        self.graph.node_count()
    }

    /// Returns the number of edges in the graph.
    pub fn num_edges(&self) -> usize {
        self.graph.edge_count()
    }
}

/// Returns the parameter vector shared by all equations, or an error if the
/// equations disagree.
fn unified_parameters(equations: &mcrl2::PbesEquations) -> Result<Vec<DataVariable>, MercError> {
    let Some(first) = equations.first() else {
        return Ok(Vec::new());
    };

    let parameters: Vec<DataVariable> = first.variable().parameters().iter().collect();
    for equation in equations.iter().skip(1) {
        let other: Vec<DataVariable> = equation.variable().parameters().iter().collect();
        if other != parameters {
            return Err(format!(
                "Equation for '{}' does not declare the same parameter vector as equation for '{}'; \
                 the symmetry detection graph currently requires every equation to share an \
                 identical (name, sort)-ordered parameter vector.\n  {}: [{}]\n  {}: [{}]",
                equation.variable().name(),
                first.variable().name(),
                first.variable().name(),
                parameters.iter().format(", "),
                equation.variable().name(),
                other.iter().format(", "),
            )
            .into());
        }
    }

    Ok(parameters)
}

/// Builds the symmetry detection graph of `pbes`.
///
/// Returns an error when the equations do not share one parameter vector (see
/// [`Pbes::unify_parameters`]), when a variable is neither a parameter nor
/// bound, or when the PBES contains a `where` clause.
pub fn build_sdg(pbes: &Pbes) -> Result<Sdg, MercError> {
    let equations = pbes.equations();
    let parameters = unified_parameters(&equations)?;

    let mut builder = SdgBuilder::new(&parameters);

    // Allocate the parameter vertices first, so `NodeIndex(k)` is parameter `k`.
    for (k, parameter) in parameters.iter().enumerate() {
        let colour = VertexColour::Parameter(parameter.sort().protect());
        builder.add_vertex(SdgVertex::Parameter(k), colour);
    }
    debug_assert_eq!(builder.graph.node_count(), parameters.len());

    let n = parameters.len();
    debug!("SDG build: {} parameter(s): [{}]", n, parameters.iter().format(", "));

    for (e, equation) in equations.iter().enumerate() {
        let formula = equation.formula();

        let rhs = builder.visit(formula.copy())?;
        builder.mark_rhs(rhs, e);

        debug!(
            "SDG build: equation {} '{}' — {} vertices so far",
            e,
            equation.variable().name(),
            builder.graph.node_count()
        );
    }

    debug_assert!(builder.scope.is_empty(), "every binder must have been left again");

    // Postconditions.
    debug_assert_eq!(builder.colours.len(), builder.rhs.len());
    debug_assert_eq!(builder.colours.len(), builder.graph.node_count());
    for k in 0..n {
        debug_assert!(
            matches!(builder.colours[k], VertexColour::Parameter(_)),
            "the first n vertices must be exactly the parameter vertices"
        );
    }
    for (index, colour) in builder.colours.iter().enumerate().skip(n) {
        debug_assert!(
            !matches!(colour, VertexColour::Parameter(_)),
            "no vertex beyond the first n may be coloured Parameter (found at index {index})"
        );
    }
    for node in builder.graph.node_indices() {
        debug_assert!(
            builder.graph.find_edge(node, node).is_none(),
            "the SDG must not contain self-loops"
        );
    }

    Ok(Sdg {
        graph: builder.graph,
        colours: builder.colours,
        rhs: builder.rhs,
        parameters,
        equation_names: equations.iter().map(|eq| eq.variable().name().to_string()).collect(),
    })
}

/// Two terms are the same vertex iff they have the same key.
///
/// The key refers to the children by their vertex, so it identifies a term by
/// its structure. Since bound variables are de Bruijn indices, alpha-equivalent
/// terms have the same key.
#[derive(Clone, PartialEq, Eq, Hash)]
enum VertexKey {
    /// The colour and the edges to the children. For a leaf the colour alone
    /// identifies the term.
    Node {
        colour: VertexColour,
        children: Vec<(NodeIndex, EdgeColour)>,
    },

    /// A PVI, whose arguments are connected through the update vertices.
    Pvi { name: String, arguments: Vec<NodeIndex> },
}

/// Builds up an [`Sdg`] incrementally by walking a PBES's right-hand sides.
struct SdgBuilder {
    /// The graph under construction; becomes [`Sdg::graph`].
    graph: UnGraph<SdgVertex, EdgeColour>,

    /// `C(v)` per vertex, indexed by `NodeIndex::index()`.
    colours: Vec<VertexColour>,

    /// `C_RHS(v)` per vertex, indexed by `NodeIndex::index()`.
    rhs: Vec<BTreeSet<usize>>,

    /// The vertex of every key.
    interned: HashMap<VertexKey, NodeIndex>,

    /// The bound variables currently in scope.
    scope: BindingScope<DataVariable>,

    /// The index of every unified PBES parameter, which is also its vertex.
    parameter_index: HashMap<DataVariable, usize>,
}

/// The children of a non-commutative operator, see [`EdgeColour::Argument`].
fn by_position(arguments: &[NodeIndex]) -> Vec<(NodeIndex, EdgeColour)> {
    let mut by_child: BTreeMap<NodeIndex, BTreeSet<usize>> = BTreeMap::new();
    for (position, &child) in arguments.iter().enumerate() {
        by_child.entry(child).or_default().insert(position + 1);
    }

    by_child
        .into_iter()
        .map(|(child, positions)| (child, EdgeColour::Argument(positions)))
        .collect()
}

/// The children of a commutative operator, without positions.
fn uncoloured(arguments: &[NodeIndex]) -> Vec<(NodeIndex, EdgeColour)> {
    arguments
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|child| (child, EdgeColour::Uncoloured))
        .collect()
}

impl SdgBuilder {
    fn new(parameters: &[DataVariable]) -> Self {
        SdgBuilder {
            graph: UnGraph::new_undirected(),
            colours: Vec::new(),
            rhs: Vec::new(),
            interned: HashMap::new(),
            scope: BindingScope::new(),
            parameter_index: parameters.iter().cloned().enumerate().map(|(k, p)| (p, k)).collect(),
        }
    }

    /// Adds a fresh vertex.
    fn add_vertex(&mut self, vertex: SdgVertex, colour: VertexColour) -> NodeIndex {
        let index = self.graph.add_node(vertex);
        debug_assert_eq!(index.index(), self.colours.len());
        self.colours.push(colour);
        self.rhs.push(BTreeSet::new());
        index
    }

    /// Records that the vertex `node` is the right-hand side of `equation`.
    fn mark_rhs(&mut self, node: NodeIndex, equation: usize) {
        self.rhs[node.index()].insert(equation);
    }

    /// Adds an edge, or merges the colour into an existing `(u, v)` edge.
    fn add_or_merge_edge(&mut self, u: NodeIndex, v: NodeIndex, colour: EdgeColour) {
        if let Some(edge) = self.graph.find_edge(u, v) {
            let existing = self
                .graph
                .edge_weight_mut(edge)
                .expect("find_edge returned a valid edge index");
            trace!(
                "edge: merge v{}--v{} colour {:?} into {:?}",
                u.index(),
                v.index(),
                colour,
                existing
            );
            *existing = merge_edge_colour(existing.clone(), colour);
        } else {
            trace!("edge: add   v{}--v{} colour {:?}", u.index(), v.index(), colour);
            self.graph.add_edge(u, v, colour);
        }
    }

    /// Returns the vertex for `key`, adding it when it does not exist yet, and
    /// whether it is new.
    fn intern(
        &mut self,
        key: VertexKey,
        term: &PbesExpression,
        colour: VertexColour,
        children: &[(NodeIndex, EdgeColour)],
    ) -> (NodeIndex, bool) {
        if let Some(&node) = self.interned.get(&key) {
            trace!("visit: reused v{} for `{}`", node.index(), term);
            return (node, false);
        }

        let node = self.add_vertex(SdgVertex::Term(term.clone()), colour.clone());
        trace!("visit: new   v{} {:?} for `{}`", node.index(), colour, term);
        for (child, edge_colour) in children {
            self.add_or_merge_edge(node, *child, edge_colour.clone());
        }

        self.interned.insert(key, node);
        (node, true)
    }

    /// Returns the vertex with the given colour and children.
    fn intern_node(
        &mut self,
        term: &PbesExpression,
        colour: VertexColour,
        children: Vec<(NodeIndex, EdgeColour)>,
    ) -> NodeIndex {
        let key = VertexKey::Node {
            colour: colour.clone(),
            children: children.clone(),
        };
        self.intern(key, term, colour, &children).0
    }

    /// Visits a term below a binder of `variables`, returning the vertex of its body.
    fn visit_binder<I>(&mut self, variables: I, body: PbesExpressionRef<'_>) -> Result<NodeIndex, MercError>
    where
        I: IntoIterator<Item = DataVariable>,
    {
        let pushed = self.scope.push(variables);
        let result = self.visit(body);
        self.scope.pop(pushed);
        result
    }

    /// Returns the vertex of `term`, creating the vertices of its subterms
    /// first, following Definition 6's `sub#`.
    fn visit(&mut self, term: PbesExpressionRef<'_>) -> Result<NodeIndex, MercError> {
        let term = term.protect();
        let r: ATermRef<'_> = term.copy().into();

        if is_pbes_and(&r) || is_pbes_or(&r) {
            // Flatten the chain `a && b && c` into one vertex with three
            // children. Both connectives are idempotent.
            let (connective, colour) = if is_pbes_and(&r) {
                (PbesConnective::And, Connective::And)
            } else {
                (PbesConnective::Or, Connective::Or)
            };

            let mut stack = PbesFlattenStack::new();
            let mut arguments = Vec::new();
            for leaf in PbesFlattenIter::new(term.copy(), connective, &mut stack) {
                arguments.push(self.visit(leaf)?);
            }

            Ok(self.intern_node(&term, VertexColour::Connective(colour), uncoloured(&arguments)))
        } else if is_pbes_imp(&r) {
            let imp = PbesImpRef::from(r);
            let lhs = self.visit(imp.lhs())?;
            let rhs = self.visit(imp.rhs())?;

            Ok(self.intern_node(
                &term,
                VertexColour::Connective(Connective::Imp),
                by_position(&[lhs, rhs]),
            ))
        } else if is_pbes_not(&r) {
            let not = PbesNotRef::from(r);
            let body = self.visit(not.body())?;

            Ok(self.intern_node(&term, VertexColour::Connective(Connective::Not), uncoloured(&[body])))
        } else if is_pbes_forall(&r) {
            let forall = PbesForallRef::from(r);
            let sorts = forall.variables().iter().map(|v| v.sort().protect()).collect();
            let body = self.visit_binder(forall.variables().iter(), forall.body())?;

            Ok(self.intern_node(
                &term,
                VertexColour::Quantifier(Quantifier::Forall, sorts),
                uncoloured(&[body]),
            ))
        } else if is_pbes_exists(&r) {
            let exists = PbesExistsRef::from(r);
            let sorts = exists.variables().iter().map(|v| v.sort().protect()).collect();
            let body = self.visit_binder(exists.variables().iter(), exists.body())?;

            Ok(self.intern_node(
                &term,
                VertexColour::Quantifier(Quantifier::Exists, sorts),
                uncoloured(&[body]),
            ))
        } else if is_pbes_propositional_variable_instantiation(&r) {
            self.visit_pvi(&term)
        } else if is_variable(&r) {
            let variable: DataVariable = DataVariableRef::from(r).protect();

            // The innermost binder wins, also over a parameter of the same name.
            if let Some(index) = self.scope.index_of(&variable) {
                let sort = variable.sort().protect();
                Ok(self.intern_node(&term, VertexColour::BoundVariable { sort, index }, Vec::new()))
            } else if let Some(&k) = self.parameter_index.get(&variable) {
                Ok(NodeIndex::new(k))
            } else {
                Err(format!(
                    "variable '{variable}' is neither a parameter nor bound by an enclosing quantifier; \
                     the symmetry detection graph requires a closed PBES"
                )
                .into())
            }
        } else if is_application(&r) {
            let application = DataApplicationRef::from(r.copy());
            let head: DataFunctionSymbol = application.data_function_symbol().protect();
            let name = head.name().to_string();
            let arity = application.data_arguments().len();

            if is_flat_operator(&name, arity) {
                let expr: DataExpressionRef<'_> = r.into();
                let leaves = flatten_associative(&expr, |t| {
                    is_application(t) && DataApplicationRef::from(t.copy()).data_function_symbol().name() == name
                });

                let mut multiplicity: BTreeMap<NodeIndex, usize> = BTreeMap::new();
                for leaf in &leaves {
                    let child = self.visit(leaf.copy().into())?;
                    *multiplicity.entry(child).or_default() += 1;
                }

                let idempotent = IDEMPOTENT_FUNCTION_SYMBOLS.contains(&name.as_str());
                let children = multiplicity
                    .into_iter()
                    .map(|(child, count)| {
                        let colour = if idempotent {
                            EdgeColour::Uncoloured
                        } else {
                            EdgeColour::Multiplicity(count)
                        };
                        (child, colour)
                    })
                    .collect();

                Ok(self.intern_node(&term, VertexColour::Function(head), children))
            } else {
                let mut arguments = Vec::new();
                for argument in application.data_arguments() {
                    arguments.push(self.visit(argument.into())?);
                }

                let children = if is_commutative(&name, arity) {
                    uncoloured(&arguments)
                } else {
                    by_position(&arguments)
                };
                Ok(self.intern_node(&term, VertexColour::Function(head), children))
            }
        } else if is_function_symbol(&r) {
            let colour = VertexColour::Function(DataFunctionSymbolRef::from(r).protect());
            Ok(self.intern_node(&term, colour, Vec::new()))
        } else if is_machine_number(&r) {
            let number = DataMachineNumberRef::from(r);
            let colour = VertexColour::MachineNumber(number.value());
            Ok(self.intern_node(&term, colour, Vec::new()))
        } else if is_untyped_identifier(&r) {
            let colour = VertexColour::Untyped(format!("{r:?}"));
            Ok(self.intern_node(&term, colour, Vec::new()))
        } else if is_abstraction(&r) {
            let abstraction = DataAbstractionRef::from(r.copy());
            let sorts: Vec<SortExpression> = abstraction.variables().iter().map(|v| v.sort().protect()).collect();
            let bo = abstraction.binding_operator();
            let quantifier = if bo.is_forall() {
                Quantifier::Forall
            } else if bo.is_exists() {
                Quantifier::Exists
            } else if bo.is_lambda() {
                Quantifier::Lambda
            } else {
                Quantifier::Comprehension
            };

            let body = self.visit_binder(abstraction.variables().iter(), abstraction.body().into())?;
            Ok(self.intern_node(&term, VertexColour::Quantifier(quantifier, sorts), uncoloured(&[body])))
        } else if is_where_clause(&r) {
            Err(MercError::from(
                "where clauses are not supported in the symmetry detection graph construction",
            ))
        } else {
            unreachable!("Unknown PBES/data expression kind for term {:?}", r)
        }
    }

    /// Returns the vertex of a PVI. A new PVI also gets its update vertices
    /// `X_{i,k}`, connected to the PVI, the `k`-th argument and parameter `d_k`.
    ///
    /// Unlike the paper, equal PVIs share one set of update vertices; duplicate
    /// sets would only add automorphisms that are the identity on parameters.
    fn visit_pvi(&mut self, term: &PbesExpression) -> Result<NodeIndex, MercError> {
        let r: ATermRef<'_> = term.copy().into();
        let pvi = PbesPropositionalVariableInstantiationRef::from(r);
        let n = self.parameter_index.len();

        let arguments: Vec<PbesExpression> = pvi.arguments().iter().map(PbesExpression::from).collect();
        if arguments.len() != n {
            return Err(format!(
                "Predicate variable instance '{}' has {} argument(s), but the unified parameter \
                 vector has {} parameter(s); every predicate variable instance must supply exactly \
                 one argument per parameter.",
                pvi.name(),
                arguments.len(),
                n
            )
            .into());
        }

        let data_nodes = arguments
            .iter()
            .map(|argument| self.visit(argument.copy()))
            .collect::<Result<Vec<_>, _>>()?;

        let name = pvi.name().to_string();
        let key = VertexKey::Pvi {
            name: name.clone(),
            arguments: data_nodes.clone(),
        };
        let (pvi_node, is_new) = self.intern(key, term, VertexColour::Pvi(name), &[]);
        if !is_new {
            return Ok(pvi_node);
        }

        debug!(
            "update vertices: pvi '{}' (v{}) — {} parameter(s)",
            pvi.name(),
            pvi_node.index(),
            n
        );

        for (k, &data_node) in data_nodes.iter().enumerate() {
            let update_node = self.add_vertex(
                SdgVertex::Update {
                    pvi: pvi_node.index(),
                    parameter: k,
                },
                VertexColour::Update,
            );
            trace!("update vertex v{} X_({},{})", update_node.index(), pvi_node.index(), k);

            let par_node = NodeIndex::new(k);

            let mut by_target: BTreeMap<NodeIndex, BTreeSet<UpdateRole>> = BTreeMap::new();
            by_target.entry(pvi_node).or_default().insert(UpdateRole::Pvi);
            by_target.entry(data_node).or_default().insert(UpdateRole::Data);
            by_target.entry(par_node).or_default().insert(UpdateRole::Par);

            for (target, roles) in by_target {
                self.add_or_merge_edge(update_node, target, EdgeColour::Update(roles));
            }
        }

        Ok(pvi_node)
    }
}

/// Merges the colours of two edges between the same vertices.
fn merge_edge_colour(a: EdgeColour, b: EdgeColour) -> EdgeColour {
    match (a, b) {
        (EdgeColour::Argument(mut xs), EdgeColour::Argument(ys)) => {
            xs.extend(ys);
            EdgeColour::Argument(xs)
        }
        (EdgeColour::Update(mut xs), EdgeColour::Update(ys)) => {
            xs.extend(ys);
            EdgeColour::Update(xs)
        }
        (EdgeColour::Uncoloured, EdgeColour::Uncoloured) => EdgeColour::Uncoloured,
        (a, b) => unreachable!(
            "Cannot merge incompatible edge colours {:?} and {:?} on the same vertex pair",
            a, b
        ),
    }
}

/// The SDG in the form GAP's Digraphs package expects: a symmetric digraph
/// (every undirected edge as two opposite directed arcs) with 1-based points.
pub struct GapGraph {
    /// The 1-based out-neighbours of vertex `u+1`.
    out_neighbours: Vec<Vec<usize>>,
    /// The colour of every arc in `out_neighbours`.
    edge_colours: Vec<Vec<usize>>,
    vertex_colours: Vec<usize>,
    /// The first `num_parameters` vertices are the parameters.
    pub num_parameters: usize,
}

impl Sdg {
    /// Returns the dense (1-based) colour of every vertex, including `C_RHS`,
    /// and of every edge, as handed to GAP.
    fn dense_colours(&self) -> (Vec<usize>, Vec<usize>) {
        let mut vc_map: HashMap<(&VertexColour, &BTreeSet<usize>), usize> = HashMap::new();
        let mut vertex_colours = vec![0usize; self.graph.node_count()];

        for node in self.graph.node_indices() {
            let key = (&self.colours[node.index()], &self.rhs[node.index()]);
            let next = vc_map.len() + 1;
            vertex_colours[node.index()] = *vc_map.entry(key).or_insert(next);
        }

        let mut ec_map: HashMap<&EdgeColour, usize> = HashMap::new();
        let mut edge_colours = vec![0usize; self.graph.edge_count()];

        for edge in self.graph.edge_indices() {
            let key = self.graph.edge_weight(edge).unwrap();
            let next = ec_map.len() + 1;
            edge_colours[edge.index()] = *ec_map.entry(key).or_insert(next);
        }

        (vertex_colours, edge_colours)
    }

    pub fn to_gap_graph(&self) -> GapGraph {
        let n = self.graph.node_count();
        let (vertex_colours, dense_edge_colours) = self.dense_colours();

        let mut out_neighbours = vec![Vec::new(); n];
        let mut edge_colours = vec![Vec::new(); n];

        for edge in self.graph.edge_indices() {
            let (u, v) = self.graph.edge_endpoints(edge).unwrap();
            let dense_ec = dense_edge_colours[edge.index()];
            out_neighbours[u.index()].push(v.index() + 1);
            edge_colours[u.index()].push(dense_ec);
            out_neighbours[v.index()].push(u.index() + 1);
            edge_colours[v.index()].push(dense_ec);
        }

        debug_assert!(
            out_neighbours
                .iter()
                .zip(&edge_colours)
                .all(|(nb, ec)| nb.len() == ec.len()),
            "out_neighbours and edge_colours must be positionally aligned"
        );
        debug_assert!(
            out_neighbours
                .iter()
                .enumerate()
                .all(|(u, nb)| { nb.iter().zip(&edge_colours[u]).collect::<HashSet<_>>().len() == nb.len() }),
            "no two arcs from the same source may share both target and colour"
        );

        GapGraph {
            out_neighbours,
            edge_colours,
            vertex_colours,
            num_parameters: self.parameters.len(),
        }
    }
}

/// Writes the SDG as a Graphviz DOT file. Vertex `p{k}` is GAP point `k`.
pub fn write_dot<W>(sdg: &Sdg, w: &mut W) -> Result<(), MercError>
where
    W: Write,
{
    writeln!(w, "graph sdg {{")?;
    writeln!(w, "  node [style=filled];")?;

    for node in sdg.graph.node_indices() {
        let i = node.index();
        let point = i + 1;
        let vc = &sdg.colours[i];
        let rhs = format!("rhs {{{}}}", sdg.rhs[i].iter().format(","));

        let label = match vc {
            VertexColour::Parameter(_) => sdg.parameters[i].name().to_string(),
            VertexColour::Update => String::new(),
            VertexColour::Pvi(name) => name.to_string(),
            VertexColour::BoundVariable { index, .. } => format!("#{index}"),
            _ => vc.to_string(),
        };

        let shape = match vc {
            VertexColour::Parameter(_) => "box",
            VertexColour::Update => "diamond",
            VertexColour::Pvi(_) => "hexagon",
            VertexColour::Quantifier(..) => "parallelogram",
            _ => "ellipse",
        };

        let fill = vc.dot_fill_colour();
        if matches!(vc, VertexColour::Update) {
            writeln!(
                w,
                "  p{point} [label=\"\", xlabel=\"{rhs}\", shape=diamond, fillcolor=\"{fill}\", width=0.2, height=0.2, fixedsize=true];"
            )?;
        } else {
            writeln!(
                w,
                "  p{point} [label=\"{label}\\n{rhs}\", shape={shape}, fillcolor=\"{fill}\"];"
            )?;
        }
    }

    for edge in sdg.graph.edge_indices() {
        let (u, v) = sdg.graph.edge_endpoints(edge).unwrap();
        let colour = sdg.graph.edge_weight(edge).unwrap();
        let elabel = colour.to_string();
        let style = if matches!(colour, EdgeColour::Update(_)) {
            "style=dashed, "
        } else {
            ""
        };

        writeln!(
            w,
            "  p{} -- p{} [{style}label=\"{elabel}\"];",
            u.index() + 1,
            v.index() + 1
        )?;
    }

    writeln!(w, "}}")?;
    Ok(())
}

/// petgraph only implements the 4-byte N(n) encoding; the format supports up to 68_719_476_735.
const GRAPH6_MAX_NODES: usize = 258_047;

/// Renders the SDG in graph6 format, which cannot represent the colours.
pub fn graph6_string(sdg: &Sdg) -> Result<String, MercError> {
    if sdg.graph.node_count() > GRAPH6_MAX_NODES {
        return Err(
            format!("petgraph's graph6 encoder does not support graphs over {GRAPH6_MAX_NODES} vertices").into(),
        );
    }
    Ok(sdg.graph.graph6_string())
}

/// Generates a GAP script that computes `Aut(G)`, restricts its generators to
/// the parameters, and prints them between `SDG-BEGIN`/`SDG-END` sentinels.
///
/// Statements end in `;;` because a single `;` echoes the value on stdin. The
/// sentinels detect syntax errors, after which GAP still exits with 0.
fn gap_script(graph: &GapGraph) -> String {
    let n = graph.num_parameters;
    let num_vertices = graph.out_neighbours.len();

    let neighbours_str = (0..num_vertices)
        .map(|u| {
            if graph.out_neighbours[u].is_empty() {
                "[]".to_string()
            } else {
                format!("[{}]", graph.out_neighbours[u].iter().join(","))
            }
        })
        .join(",");

    let vc_str = graph.vertex_colours.iter().join(",");

    let ec_str = (0..num_vertices)
        .map(|u| {
            if graph.edge_colours[u].is_empty() {
                "[]".to_string()
            } else {
                format!("[{}]", graph.edge_colours[u].iter().join(","))
            }
        })
        .join(",");

    format!(
        r#"SetPrintFormattingStatus("*stdout*", false);;
if LoadPackage("digraphs") = fail then
  Error("the GAP package 'Digraphs' could not be loaded; install it from https://digraphs.github.io/Digraphs/");
fi;;
D := Digraph([{neighbours_str}]);;
vcolours := [{vc_str}];;
ecolours := [{ec_str}];;
A := AutomorphismGroup(D, vcolours, ecolours);;
R := List(GeneratorsOfGroup(A), g -> RestrictedPerm(g, [1..{n}]));;
P := GroupByGenerators(R, ());;
Print("SDG-BEGIN\n");;
Print("order ", Size(A), "\n");;
Print("restricted ", Size(P), "\n");;
for g in GeneratorsOfGroup(P) do
  for i in [1..{n}] do Print(i^g, " "); od;
  Print("\n");
od;;
Print("SDG-END\n");;
QUIT;;
"#
    )
}

/// Configuration for invoking the external GAP process.
pub struct GapConfig {
    /// Path or name of the GAP executable (default: `"gap"` on `$PATH`).
    pub executable: String,
    /// If set, the generated GAP script is also written to this file.
    pub dump_script: Option<PathBuf>,
}

impl Default for GapConfig {
    fn default() -> Self {
        GapConfig {
            executable: "gap".to_string(),
            dump_script: None,
        }
    }
}

/// Runs GAP on `script` and returns its stdout.
///
/// Uses `-q` (no banner), `-A` (no package autoloading) and `--quitonbreak`
/// (non-zero exit on errors). Do not add `-T`, which defeats `--quitonbreak`,
/// or `-r`, which hides the user's `~/.gap` where Digraphs is often installed.
pub fn run_gap(script: &str, config: &GapConfig) -> Result<String, MercError> {
    if let Some(path) = &config.dump_script {
        fs::write(path, script)
            .map_err(|e| MercError::from(format!("failed to write GAP script to '{}': {}", path.display(), e)))?;
    }

    let output = duct::cmd(&*config.executable, ["-q", "-A", "--quitonbreak"])
        .stdin_bytes(script.to_owned())
        .stdout_capture()
        .stderr_capture()
        .unchecked()
        .run()
        .map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                MercError::from(format!(
                    "GAP executable '{}' not found; install GAP from https://www.gap-system.org/ \
                     or pass --gap-path to specify its location",
                    config.executable
                ))
            } else {
                MercError::from(e)
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut msg = format!("GAP exited with {}", output.status);
        if stderr.contains("digraphs") || stderr.contains("Digraphs") {
            msg.push_str(
                "; the Digraphs package may not be installed — \
                 see https://digraphs.github.io/Digraphs/",
            );
        } else if !stderr.trim().is_empty() {
            msg.push_str(&format!(": {}", stderr.trim()));
        }
        return Err(msg.into());
    }

    String::from_utf8(output.stdout).map_err(|e| MercError::from(e.to_string()))
}

/// Parses the output of [`gap_script`] into `(|Aut(G)|, |Sym(pbes)|,
/// generators)`, converting GAP's 1-based images to 0-based permutations.
fn parse_gap_output(stdout: &str, num_parameters: usize) -> Result<(u128, u128, Vec<Permutation>), MercError> {
    let begin_pos = stdout.find("SDG-BEGIN").ok_or_else(|| {
        MercError::from("GAP output is missing 'SDG-BEGIN' sentinel — check for syntax errors in the generated script")
    })?;
    let after_begin = &stdout[begin_pos + "SDG-BEGIN".len()..];
    let end_pos = after_begin
        .find("SDG-END")
        .ok_or_else(|| MercError::from("GAP output is missing 'SDG-END' sentinel"))?;
    let inner = after_begin[..end_pos].trim();

    let mut lines = inner.lines();

    let aut_order: u128 = lines
        .next()
        .and_then(|l| l.strip_prefix("order "))
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| MercError::from("expected 'order <n>' as first line inside sentinels"))?;

    let sym_order: u128 = lines
        .next()
        .and_then(|l| l.strip_prefix("restricted "))
        .and_then(|s| s.trim().parse().ok())
        .ok_or_else(|| MercError::from("expected 'restricted <n>' as second line inside sentinels"))?;

    let mut generators = Vec::new();
    for (line_no, line) in lines.enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let images: Vec<usize> = line
            .split_whitespace()
            .map(|s| {
                let v = s
                    .parse::<usize>()
                    .map_err(|_| MercError::from(format!("invalid image '{}' on generator line {}", s, line_no)))?;
                v.checked_sub(1).ok_or_else(|| {
                    MercError::from(format!(
                        "invalid image '{}' on generator line {} (expected >= 1)",
                        s, line_no
                    ))
                })
            })
            .collect::<Result<_, _>>()?;

        if images.len() != num_parameters {
            return Err(format!(
                "generator line {} has {} images but expected {} (num_parameters)",
                line_no,
                images.len(),
                num_parameters
            )
            .into());
        }

        let mapping: Vec<(usize, usize)> = images.into_iter().enumerate().filter(|(from, to)| from != to).collect();

        if !mapping.is_empty() {
            generators.push(Permutation::from_mapping(mapping));
        }
    }

    Ok((aut_order, sym_order, generators))
}

/// Result returned by [`graph_symmetries`].
pub struct GraphSymmetryResult {
    /// The symmetry detection graph the automorphisms were computed on.
    pub sdg: Sdg,

    /// `|Aut(G)|`, the order of the automorphism group of the whole SDG.
    pub automorphism_group_order: u128,

    /// `|Sym(pbes)|`, the order after restricting to the parameter vertices.
    pub symmetry_group_order: u128,

    /// Generators of `Sym(pbes)`, as permutations of the parameter indices.
    pub generators: Vec<Permutation>,
}

/// Computes the parameter symmetries of `pbes` as the automorphisms of its
/// symmetry detection graph, using GAP.
pub fn graph_symmetries(pbes: &Pbes, config: &GapConfig) -> Result<GraphSymmetryResult, MercError> {
    // Unify a copy, so the caller's PBES is unchanged.
    let mut pbes = pbes.clone();
    pbes.unify_parameters(UNIFY_IGNORE_CE_EQUATIONS, UNIFY_RESET_PARAMETERS)?;
    let sdg = build_sdg(&pbes)?;
    info!(
        "SDG: {} vertices, {} edges, {} parameters",
        sdg.num_vertices(),
        sdg.num_edges(),
        sdg.num_parameters()
    );
    debug!("SDG equation names: [{}]", sdg.equation_names.iter().format(", "));

    let gap_graph = sdg.to_gap_graph();
    let script = gap_script(&gap_graph);
    let stdout = run_gap(&script, config)?;
    let (aut_order, sym_order, generators) = parse_gap_output(&stdout, gap_graph.num_parameters)?;

    info!(
        "|Aut(G)| = {}, |Sym(pbes)| = {}, {} generator(s)",
        aut_order,
        sym_order,
        generators.len()
    );

    Ok(GraphSymmetryResult {
        sdg,
        automorphism_group_order: aut_order,
        symmetry_group_order: sym_order,
        generators,
    })
}

#[cfg(test)]
mod tests {
    use mcrl2::Pbes;
    use merc_utilities::test_logger;
    use petgraph::graph::NodeIndex;
    use petgraph::visit::EdgeRef;
    use std::sync::OnceLock;

    use test_case::test_case;

    use super::EdgeColour;
    use super::GapConfig;
    use super::GraphSymmetryResult;
    use super::Quantifier;
    use super::UpdateRole;
    use super::VertexColour;
    use super::build_sdg;
    use super::gap_script;
    use super::graph_symmetries;
    use super::parse_gap_output;
    use super::run_gap;

    /// Returns `true` when GAP with the Digraphs package is usable.
    /// Cached so the probe script runs at most once per test process.
    fn gap_with_digraphs_available() -> bool {
        static AVAILABLE: OnceLock<bool> = OnceLock::new();
        *AVAILABLE.get_or_init(|| {
            duct::cmd("gap", ["-q", "-A", "--quitonbreak"])
                .stdin_bytes("if LoadPackage(\"digraphs\") = fail then QUIT_GAP(1); fi;; QUIT_GAP(0);;")
                .stdout_null()
                .stderr_null()
                .unchecked()
                .run()
                .map(|o| o.status.success())
                .unwrap_or(false)
        })
    }

    /// Runs `graph_symmetries` on the given PBES source, skipping if GAP or
    /// Digraphs is unavailable.
    fn check_gap_symmetries(source: &str) -> Option<GraphSymmetryResult> {
        if !gap_with_digraphs_available() {
            return None;
        }
        let pbes = mcrl2::Pbes::from_text(source).unwrap();
        Some(graph_symmetries(&pbes, &GapConfig::default()).unwrap())
    }

    #[test_case(include_str!("../../../../../examples/pbes/a.text.pbes"); "a")]
    #[test_case(include_str!("../../../../../examples/pbes/b.text.pbes"); "b")]
    #[test_case(include_str!("../../../../../examples/pbes/c.text.pbes"); "c")]
    #[test_case(include_str!("../../../../../examples/pbes/alloc3.text.pbes");  "alloc3")]
    #[test_case(include_str!("../../../../../examples/pbes/alloc7.text.pbes");  "alloc7")]
    #[test_case(include_str!("../../../../../examples/pbes/alloc9.text.pbes");  "alloc9")]
    #[test_case(include_str!("../../../../../examples/pbes/dining8.text.pbes"); "dining8")]
    fn test_gap_symmetries(source: &str) {
        check_gap_symmetries(source);
    }

    /// Checks the graph structure of the SDG built from `c.text.pbes`, which
    /// has 4 parameters.
    #[test]
    fn test_c_pbes_parameter_vertices() {
        test_logger();
        let pbes = Pbes::from_text(include_str!("../../../../../examples/pbes/c.text.pbes")).unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        assert_eq!(sdg.num_parameters(), 4, "c.text.pbes has 4 parameters");
        for k in 0..4 {
            assert!(
                matches!(sdg.colours[k], VertexColour::Parameter(_)),
                "vertex {k} should be the k'th parameter"
            );
        }
        // No other vertex may be coloured Parameter.
        assert!(
            sdg.colours
                .iter()
                .skip(4)
                .all(|c| !matches!(c, VertexColour::Parameter(_)))
        );

        assert!(sdg.num_vertices() > 4, "there should be vertices beyond the parameters");
        assert!(sdg.num_edges() > 0);
    }

    #[test]
    fn test_a_and_b_pbes_build_without_error() {
        test_logger();
        for source in [
            include_str!("../../../../../examples/pbes/a.text.pbes"),
            include_str!("../../../../../examples/pbes/b.text.pbes"),
        ] {
            let pbes = Pbes::from_text(source).unwrap();
            build_sdg(&pbes).unwrap();
        }
    }

    /// Identical subterms across two different equations collapse to a
    /// single vertex, and `C_RHS` only marks the vertex that *is* a right-hand
    /// side, not the subterms reachable from it.
    #[test]
    fn test_identical_subterms_share_one_vertex_across_equations() {
        test_logger();
        let pbes = Pbes::from_text(
            "pbes mu X(n: Nat) = val(n == 0) && Y(n);
                  mu Y(n: Nat) = val(n == 0) && X(n);
             init X(0);",
        )
        .unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        let condition_node = sdg
            .graph
            .node_indices()
            .find(
                |&index| matches!(&sdg.colours[index.index()], VertexColour::Function(symbol) if symbol.name() == "=="),
            )
            .expect("there should be exactly one vertex for the '==' application");
        assert!(
            sdg.rhs[condition_node.index()].is_empty(),
            "a shared subterm that is not itself a right-hand side has an empty C_RHS"
        );

        // X and Y have different right-hand sides (`... && Y(n)` vs `... && X(n)`).
        let marked: Vec<_> = sdg.rhs.iter().filter(|set| !set.is_empty()).collect();
        assert_eq!(marked.len(), 2, "each right-hand side is its own vertex");
    }

    /// Equations with an identical right-hand side share a vertex whose
    /// `C_RHS` is the set of all of them.
    #[test]
    fn test_identical_right_hand_sides_share_c_rhs() {
        test_logger();
        let pbes = Pbes::from_text(
            "pbes mu X(n: Nat) = val(n == 0);
                  mu Y(n: Nat) = val(n == 0);
             init X(0);",
        )
        .unwrap();
        let sdg = build_sdg(&pbes).unwrap();
        assert!(sdg.rhs.iter().any(|set| *set == [0, 1].into_iter().collect()));
    }

    /// Update vertices are never deduplicated, even when two `(equation,
    /// pvi, parameter)` triples would otherwise look structurally identical.
    #[test]
    fn test_update_vertices_are_never_deduplicated() {
        test_logger();
        let pbes = Pbes::from_text(
            "pbes mu X(n: Nat) = Y(n) && Y(n);
                  mu Y(n: Nat) = val(n == 0);
             init X(0);",
        )
        .unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        // Two distinct PVI occurrences `Y(n)` in X's right-hand side, each
        // contributing its own update vertex X_{i,0}, even though both
        // PVIs are syntactically identical.
        let update_count = sdg
            .graph
            .node_indices()
            .filter(|&index| sdg.colours[index.index()] == VertexColour::Update)
            .count();
        assert_eq!(update_count, 1, "one PVI occurrence (post-dedup) times one parameter");
    }

    /// The head function symbol of an application is a *colour*, not a vertex:
    /// `n + n` should produce exactly two vertices, not three.
    #[test]
    fn test_head_function_symbol_is_not_a_vertex() {
        test_logger();
        let pbes = Pbes::from_text("pbes mu X(n: Nat) = val(n + n == n); init X(0);").unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        let function_names: Vec<&str> = sdg
            .colours
            .iter()
            .filter_map(|colour| match colour {
                VertexColour::Function(symbol) => Some(symbol.name()),
                _ => None,
            })
            .collect();

        // "+" and "==" should both appear as vertex colours.
        assert!(function_names.contains(&"+"));
        assert!(function_names.contains(&"=="));
    }

    /// Overloads share a name, but are different functions, so they must not
    /// share a colour.
    #[test]
    fn test_overloaded_function_symbols_have_different_colours() {
        test_logger();
        let pbes =
            Pbes::from_text("pbes mu X(n: Nat, b: Bool) = val(n == n) && val(b == b); init X(0, true);").unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        let equalities: Vec<&VertexColour> = sdg
            .colours
            .iter()
            .filter(|colour| matches!(colour, VertexColour::Function(symbol) if symbol.name() == "=="))
            .collect();

        assert_eq!(equalities.len(), 2, "one == vertex per sort");
        assert_ne!(
            equalities[0], equalities[1],
            "== at Nat and at Bool must be coloured differently"
        );
    }

    /// A non-commutative function applied twice to the *same* argument
    /// (`n - n`) produces exactly one edge to `n`, coloured with the
    /// combined position set `{1,2}`.
    #[test]
    fn test_noncommutative_repeated_argument_gets_combined_label() {
        test_logger();
        let pbes = Pbes::from_text("pbes mu X(n: Int) = val(n - n == 0); init X(0);").unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        let minus_node = sdg
            .graph
            .node_indices()
            .find(
                |&index| matches!(&sdg.colours[index.index()], VertexColour::Function(symbol) if symbol.name() == "-"),
            )
            .expect("there should be a vertex for the '-' application");

        // Use edges_connecting to count only edges between minus_node and the parameter `n`
        // (NodeIndex(0)), not all incident edges (which would include the parent `==` edge).
        let n_node = NodeIndex::new(0);
        let edges: Vec<_> = sdg.graph.edges_connecting(minus_node, n_node).collect();
        assert_eq!(edges.len(), 1, "n - n should have exactly one edge to 'n'");
        assert_eq!(*edges[0].weight(), EdgeColour::Argument([1, 2].into_iter().collect()));
    }

    /// A commutative function applied twice to the same argument (`n ==
    /// n`) still produces exactly one (uncoloured) edge.
    #[test]
    fn test_commutative_repeated_argument_stays_uncoloured() {
        test_logger();
        let pbes = Pbes::from_text("pbes mu X(n: Nat) = val(n == n); init X(0);").unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        let eq_node = sdg
            .graph
            .node_indices()
            .find(
                |&index| matches!(&sdg.colours[index.index()], VertexColour::Function(symbol) if symbol.name() == "=="),
            )
            .expect("there should be a vertex for the '==' application");

        let edges: Vec<_> = sdg.graph.edges(eq_node).collect();
        assert_eq!(edges.len(), 1, "n == n should reach 'n' via exactly one edge");
        assert_eq!(*edges[0].weight(), EdgeColour::Uncoloured);
    }

    /// A PVI that copies a parameter unchanged (`X(n)` from within `X`'s own
    /// right-hand side) makes the `data(X,i,k)` and `d_k` update-edge
    /// targets coincide: the update vertex must have exactly one combined
    /// `{Data, Par}`-coloured edge to `n`, plus a separate `{Pvi}`-coloured
    /// edge to the PVI vertex -- not three edges, and not a silently
    /// dropped role.
    #[test]
    fn test_update_edge_combines_roles_on_parameter_copy() {
        test_logger();
        let pbes = Pbes::from_text("pbes mu X(n: Nat) = X(n); init X(0);").unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        let update_node = sdg
            .graph
            .node_indices()
            .find(|&index| sdg.colours[index.index()] == VertexColour::Update)
            .expect("there should be exactly one update vertex");

        let mut edges: Vec<_> = sdg.graph.edges(update_node).map(|e| e.weight().clone()).collect();
        edges.sort_by_key(|colour| format!("{colour:?}"));

        assert_eq!(
            edges.len(),
            2,
            "expected one {{Pvi}} edge and one combined {{Data,Par}} edge"
        );
        assert!(edges.contains(&EdgeColour::Update([UpdateRole::Pvi].into_iter().collect())));
        assert!(edges.contains(&EdgeColour::Update(
            [UpdateRole::Data, UpdateRole::Par].into_iter().collect()
        )));

        // And the combined edge's target must be the parameter vertex `n`,
        // i.e. NodeIndex(0).
        let combined_target = sdg
            .graph
            .edges(update_node)
            .find(|e| *e.weight() == EdgeColour::Update([UpdateRole::Data, UpdateRole::Par].into_iter().collect()))
            .map(|e| e.target())
            .unwrap();
        assert_eq!(combined_target, NodeIndex::new(0));
    }

    /// Two equations declaring different parameter vectors are rejected
    /// with a clear error rather than silently producing a malformed graph.
    #[test]
    fn test_rejects_non_uniform_parameter_vectors() {
        test_logger();
        // Use the same arity but different parameter names so mCRL2 accepts the PBES
        // while `unified_parameters` still rejects it (n:Nat ≠ m:Nat as ATerms).
        let pbes = Pbes::from_text(
            "pbes mu X(n: Nat) = Y(n);
                  mu Y(m: Nat) = val(true);
             init X(0);",
        )
        .unwrap();

        let result = build_sdg(&pbes);
        assert!(result.is_err(), "differing parameter vectors must be rejected");
    }

    /// A bound variable may reuse the name of a parameter: the occurrences
    /// inside the binder refer to the binder, and the ones outside to the
    /// parameter, so the parameter vertex must not be merged with the bound one.
    #[test]
    fn test_bound_variable_shadowing_parameter_is_resolved_by_scope() {
        test_logger();
        let pbes = Pbes::from_text("pbes mu X(n: Nat) = val(n > 1) && exists n: Nat . val(n > 0); init X(0);").unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        let bound = sdg
            .colours
            .iter()
            .filter(|c| matches!(c, VertexColour::BoundVariable { .. }))
            .count();
        assert_eq!(bound, 1, "the n inside the binder is bound");
        assert_eq!(
            sdg.colours
                .iter()
                .filter(|c| matches!(c, VertexColour::Parameter(_)))
                .count(),
            1,
            "and the outer n is still the parameter"
        );
    }

    /// The same through a data-level binder.
    #[test]
    fn test_data_level_binder_shadowing_parameter_is_resolved_by_scope() {
        test_logger();
        let pbes = Pbes::from_text("pbes mu X(n: Nat) = val(forall n: Nat . n > 0); init X(0);").unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        assert!(
            sdg.colours
                .iter()
                .any(|c| matches!(c, VertexColour::BoundVariable { index: 0, .. }))
        );
    }

    /// Reusing a parameter's *name* with a *different* sort is also fine.
    #[test]
    fn test_accepts_bound_variable_reusing_name_with_different_sort() {
        test_logger();
        let pbes = Pbes::from_text("pbes mu X(n: Nat) = exists n: Bool . val(n); init X(0);").unwrap();

        let sdg = build_sdg(&pbes).unwrap();
        assert_eq!(sdg.num_parameters(), 1);
    }

    /// Returns the colours of the bound-variable vertices, sorted.
    fn bound_variables(sdg: &super::Sdg) -> Vec<(String, usize)> {
        let mut result: Vec<(String, usize)> = sdg
            .colours
            .iter()
            .filter_map(|c| match c {
                VertexColour::BoundVariable { sort, index } => Some((sort.to_string(), *index)),
                _ => None,
            })
            .collect();
        result.sort();
        result
    }

    /// In `forall x. forall y. x => y` the variable `x` is bound by the outer and
    /// `y` by the inner binder, so they have different indices.
    #[test]
    fn test_bound_variables_of_the_same_sort_get_different_indices() {
        test_logger();
        let pbes =
            Pbes::from_text("pbes mu X(n: Nat) = forall x: Bool . forall y: Bool . val(x) => val(y); init X(0);")
                .unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        assert_eq!(
            bound_variables(&sdg),
            vec![("Bool".to_string(), 0), ("Bool".to_string(), 1)]
        );
    }

    /// `forall x. forall y. x => y` and `forall x. forall y. y => x` are
    /// different formulas, so they must not share their body vertex. Colouring a
    /// bound variable by its sort alone makes them indistinguishable.
    #[test]
    fn test_swapped_bound_variables_are_different_vertices() {
        test_logger();
        let single =
            Pbes::from_text("pbes mu X(n: Nat) = forall x: Bool . forall y: Bool . val(x) => val(y); init X(0);")
                .unwrap();
        let both = Pbes::from_text(
            "pbes mu X(n: Nat) = (forall x: Bool . forall y: Bool . val(x) => val(y))
                              && (forall x: Bool . forall y: Bool . val(y) => val(x));
             init X(0);",
        )
        .unwrap();

        let implications = |pbes: &Pbes| {
            let sdg = build_sdg(pbes).unwrap();
            sdg.colours
                .iter()
                .filter(|c| matches!(c, VertexColour::Connective(super::Connective::Imp)))
                .count()
        };
        assert_eq!(implications(&single), 1);
        assert_eq!(implications(&both), 2, "x => y and y => x are different bodies");
    }

    /// Terms that only differ by the names of their bound variables are
    /// alpha-equivalent, and must be the same vertex.
    #[test]
    fn test_alpha_equivalent_terms_share_a_vertex() {
        test_logger();
        let pbes = Pbes::from_text(
            "pbes mu X(n: Nat) = (exists a: Nat . val(a > n)) && (exists b: Nat . val(b > n));
             init X(0);",
        )
        .unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        let exists = sdg
            .colours
            .iter()
            .filter(|c| matches!(c, VertexColour::Quantifier(Quantifier::Exists, _)))
            .count();
        assert_eq!(exists, 1, "exists a. a > n is alpha-equivalent to exists b. b > n");
    }

    /// Alpha-equivalent right-hand sides have the same `C_RHS` vertex.
    #[test]
    fn test_alpha_equivalent_right_hand_sides_share_c_rhs() {
        test_logger();
        let pbes = Pbes::from_text(
            "pbes mu X(n: Nat) = exists a: Nat . val(a > n);
                  mu Y(n: Nat) = exists b: Nat . val(b > n);
             init X(0);",
        )
        .unwrap();
        let sdg = build_sdg(&pbes).unwrap();
        assert!(sdg.rhs.iter().any(|set| *set == [0, 1].into_iter().collect()));
    }

    /// A PVI below a binder may use the bound variable as an argument.
    #[test]
    fn test_pvi_argument_may_be_a_bound_variable() {
        test_logger();
        let pbes = Pbes::from_text(
            "pbes mu X(n: Nat) = exists m: Nat . val(m > n) && X(m);
             init X(0);",
        )
        .unwrap();
        let sdg = build_sdg(&pbes).unwrap();
        assert_eq!(bound_variables(&sdg).len(), 1);
    }

    /// A bound (quantifier-scoped) variable must not be coloured the same
    /// as a PBES parameter, even when it shares a name with one -- otherwise
    /// an automorphism could conflate the two.
    #[test]
    fn test_bound_variable_is_not_coloured_as_parameter() {
        test_logger();
        // Use a different name for the bound variable so it is a distinct ATerm from the
        // parameter `n`. Under mCRL2 hash-consing, `n:Nat` and `n:Nat` are the same ATerm
        // and would therefore collapse to one vertex regardless of scope.
        let pbes = Pbes::from_text("pbes mu X(n: Nat) = exists m: Nat . val(n == m); init X(0);").unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        let parameter_count = sdg
            .colours
            .iter()
            .filter(|c| matches!(c, VertexColour::Parameter(_)))
            .count();
        assert_eq!(parameter_count, 1);

        let has_bound_variable = sdg
            .colours
            .iter()
            .any(|c| matches!(c, VertexColour::BoundVariable { .. }));
        assert!(
            has_bound_variable,
            "the quantifier-bound 'm' should be coloured BoundVariable"
        );
    }

    /// Parameters of different sorts must never be interchangeable.
    ///
    /// Function symbols are coloured by name only (`==` exists at every sort), so
    /// without the sort in the parameter colour these two parameters have
    /// isomorphic neighbourhoods and GAP reports a `Bool` <-> `Nat` swap. Applying
    /// it would hand `set_assignments` a value of the wrong sort.
    #[test]
    fn test_parameters_of_different_sorts_are_not_interchangeable() {
        test_logger();
        let pbes = Pbes::from_text("pbes nu X(b: Bool, n: Nat) = X(b, n); init X(true, 0);").unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        assert_ne!(
            sdg.colours[0], sdg.colours[1],
            "a Bool and a Nat parameter must get different colours"
        );
    }

    /// Two PVIs of different equations must get different colours: `C(X(...)) = X`
    /// and not just `pvi`.
    #[test]
    fn test_pvis_of_different_equations_have_different_colours() {
        test_logger();
        let pbes = Pbes::from_text(
            "pbes mu X(n: Nat) = Y(n) && Z(n);
                  mu Y(n: Nat) = val(n == 0);
                  mu Z(n: Nat) = val(n > 0);
             init X(0);",
        )
        .unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        let pvi_colours: Vec<&VertexColour> = sdg
            .colours
            .iter()
            .filter(|c| matches!(c, VertexColour::Pvi(_)))
            .collect();
        assert!(pvi_colours.contains(&&VertexColour::Pvi("Y".to_string())));
        assert!(pvi_colours.contains(&&VertexColour::Pvi("Z".to_string())));
    }

    /// Colouring every PVI with the same colour `pvi` is unsound: in `X = Y(n)
    /// && Z(n)` the vertices `Y(n)` and `Z(n)` then look interchangeable, so
    /// the SDG gets an automorphism swapping them even though `Y` and `Z`
    /// are defined differently. With `C(X(...)) = X` that automorphism is gone.
    #[test]
    fn test_uniform_pvi_colour_adds_spurious_automorphism() {
        test_logger();
        if !gap_with_digraphs_available() {
            return;
        }
        let source = "pbes mu X(n: Nat) = Y(n) && Z(n);
                           mu Y(n: Nat) = val(n == 0);
                           mu Z(n: Nat) = val(n > 0);
                      init X(0);";
        let result = check_gap_symmetries(source).unwrap();
        assert_eq!(
            result.automorphism_group_order, 1,
            "Y and Z differ, so nothing may be swapped"
        );

        // Recolour every PVI with the same colour, as the old `pvi` colouring did.
        let mut sdg = result.sdg;
        for colour in sdg.colours.iter_mut() {
            if matches!(colour, VertexColour::Pvi(_)) {
                *colour = VertexColour::Pvi(String::new());
            }
        }
        let gap_graph = sdg.to_gap_graph();
        let stdout = run_gap(&gap_script(&gap_graph), &GapConfig::default()).unwrap();
        let (aut_order, _, _) = parse_gap_output(&stdout, gap_graph.num_parameters).unwrap();
        assert!(
            aut_order > 1,
            "a uniform PVI colour wrongly allows swapping Y(n) and Z(n)"
        );
    }

    /// Flattening must not forget operand multiplicity for non-idempotent
    /// operators: `n + n + m` and `n + m + m` differ, so swapping `n` and `m`
    /// is not a symmetry of `n + n + m == 3`.
    #[test]
    fn test_flattened_sum_keeps_operand_multiplicity() {
        test_logger();
        if !gap_with_digraphs_available() {
            return;
        }
        let result = check_gap_symmetries("pbes mu X(n: Nat, m: Nat) = val(n + n + m == 3); init X(1, 1);").unwrap();
        assert_eq!(
            result.symmetry_group_order, 1,
            "n + n + m is not invariant under n <-> m"
        );

        let result = check_gap_symmetries("pbes mu X(n: Nat, m: Nat) = val(n + m + 0 == 3); init X(1, 1);").unwrap();
        assert_eq!(result.symmetry_group_order, 2, "n + m is invariant under n <-> m");
    }

    /// `X(n, m) = A(n) && B(m)` with `A(p) = exists x. forall y. f(x, y, p)` and
    /// `B(p) = exists x. forall y. f(y, x, p)` for `f(a, b, c) = (!a && !b) ||
    /// (!a && b && c) || (a && !b && !c)`. Then `A(p) = p` and `B(p) = !p`, so `X`
    /// means `n && !m` and swapping `n` and `m` is not a symmetry. Without
    /// de Bruijn indices the bound variables `x` and `y` cannot be told apart, so
    /// swapping `n <-> m` together with `x <-> y` is an automorphism of the SDG.
    #[test]
    fn test_swapped_bound_variable_roles_are_not_a_symmetry() {
        test_logger();
        if !gap_with_digraphs_available() {
            return;
        }
        let source = "pbes nu X(n: Bool, m: Bool) =
              (exists x: Bool . forall y: Bool . val((!x && !y) || (!x && y && n) || (x && !y && !n)))
           && (exists x: Bool . forall y: Bool . val((!y && !x) || (!y && x && m) || (y && !x && !m)));
          init X(true, false);";
        let result = check_gap_symmetries(source).unwrap();
        assert_eq!(result.symmetry_group_order, 1, "n && !m is not invariant under n <-> m");
    }

    /// A data-level binder expression (e.g. `val(exists m:Nat. n==m)`) must be
    /// assigned a `Quantifier` colour and its bound variable must be coloured
    /// `BoundVariable`, not `Parameter`.
    #[test]
    fn test_data_level_binder_does_not_panic() {
        test_logger();
        // `val(exists m: Nat . n == m)` wraps a data-level binder inside val(...)
        // so the ATerm reaching colour_of is `Binder(Exists, [m:Nat], ==(n,m))`.
        let pbes = Pbes::from_text("pbes mu X(n: Nat) = val(exists m: Nat . n == m); init X(0);").unwrap();
        let sdg = build_sdg(&pbes).unwrap();

        let has_quantifier = sdg
            .colours
            .iter()
            .any(|c| matches!(c, VertexColour::Quantifier(Quantifier::Exists, _)));
        assert!(
            has_quantifier,
            "data-level exists binder must produce a Quantifier(Exists) vertex"
        );

        let has_bound_variable = sdg
            .colours
            .iter()
            .any(|c| matches!(c, VertexColour::BoundVariable { .. }));
        assert!(
            has_bound_variable,
            "the data-level bound 'm' must be coloured BoundVariable"
        );
    }
}
