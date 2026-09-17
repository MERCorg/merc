use std::collections::HashMap;
use std::convert::Infallible;
use std::ops::ControlFlow;

use merc_syntax::ComplexSort;
use merc_syntax::ConstructorId;
use merc_syntax::DataExpr;
use merc_syntax::DataExprKind;
use merc_syntax::EqnSpec;
use merc_syntax::MapId;
use merc_syntax::SortDecl;
use merc_syntax::SortExpression;
use merc_syntax::SortExpressionKind;
use merc_syntax::SortId;
use merc_syntax::Span;
use merc_syntax::Traverse;
use merc_syntax::UntypedDataSpecification;
use merc_syntax::VarId;

use crate::DataSpecification;
use crate::EquationTyping;
use crate::ExprId;
use crate::NameTarget;
use crate::ResolvedSort;
use crate::ResolvedSortId;
use crate::TypeCheckContext;
use crate::is_system_generated_name;
use crate::unreachable_not_a_value_sort;

/// A `VarId -> declaration span` lookup, covering exactly the binders one [`TypingInfo`] query
/// needs to resolve a [`ResolvedName::Variable`] occurrence.
#[derive(Default)]
pub(crate) struct VariableSpans(HashMap<VarId, Span>);

impl VariableSpans {
    pub(crate) fn insert(&mut self, var_id: VarId, span: Span) {
        self.0.insert(var_id, span);
    }

    pub(crate) fn get(&self, var_id: &VarId) -> Option<&Span> {
        self.0.get(var_id)
    }
}

impl FromIterator<(VarId, Span)> for VariableSpans {
    fn from_iter<T: IntoIterator<Item = (VarId, Span)>>(iter: T) -> Self {
        VariableSpans(iter.into_iter().collect())
    }
}

/// The typing of a document's data specification (or of one expression): one
/// [`TypedNode`] per checked expression node, in generation order, keyed by
/// [`Span`] rather than the crate's internal `ExprId`. `ExprId` is assigned
/// over the *lowered* expression tree, which can contain nodes with no
/// counterpart in the original `DataExpr` a caller parsed, so it can't be
/// reconstructed externally.
///
/// # Caveats
///
/// - A node synthesized during lowering inherits the span of the surface
///   expression it came from, so more than one [`TypedNode`] can share a span;
///   see [`Self::at_offset`]'s tie-break rule.
/// - [`TypedNode::sort`] is reconstructed from the resolved sort, not read from
///   a declaration, and always carries [`Span::default`] — name resolution and
///   alias normalization discard or relocate a [`SortExpression`]'s original
///   span. A sort *reference* (see [`ResolvedName::Sort`]) is unaffected: its
///   span is captured before normalization ever touches it.
/// - `TypedNode::sort` is `None` for a node with no data sort at all: an
///   action/process occurrence (its declared argument sorts are a *domain*, not
///   a value sort) or a [`ResolvedName::Sort`] occurrence (not a data
///   expression at all) — unlike every other `TypedNode`, which is always built
///   from a checked [`merc_syntax::DataExpr`].
#[derive(Debug, Default, Clone)]
pub struct TypingInfo {
    nodes: Vec<TypedNode>,
}

/// The typing of a single expression node. See [`TypingInfo`]'s doc comment for the caveats on
/// `span` and `sort`.
#[derive(Debug, Clone)]
pub struct TypedNode {
    /// The node's location in the original source.
    pub span: Span,
    /// The node's inferred sort, reconstructed as a [`SortExpression`] so it can be printed.
    pub sort: Option<SortExpression>,
    /// What this node's identifier resolved to. `None` for every node that isn't an `Id`.
    pub name: Option<ResolvedName>,
}

/// What an identifier ([`TypedNode::name`]) resolved to.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ResolvedName {
    /// An equation variable, a process/PBES/PRES parameter, or a `sum`/`dist`/quantifier binder.
    Variable {
        name: String,
        /// Resolved via the `VariableSpans` map in scope where this node was built.
        declaration: Option<Span>,
    },
    /// A user-declared constructor.
    Constructor {
        name: String,
        id: ConstructorId,
        /// The declaration's span, when it has one.
        declaration: Option<Span>,
    },
    /// A user-declared mapping.
    Mapping {
        name: String,
        id: MapId,
        declaration: Option<Span>,
    },
    /// A declared Appendix-B symbol (`succ`, `@c0`, …) — not a user declaration.
    SystemDefined {
        name: String,
        /// `None` when checked against a discarded `SourceMap`.
        declaration: Option<Span>,
    },
    /// A polymorphic built-in (`==`, `!=`, …), whose concrete meaning follows
    /// from the inferred argument sorts rather than one declaration.
    Builtin { name: String },
    /// A declared action.
    Action { name: String, declaration: Option<Span> },
    /// A declared process.
    Process { name: String, declaration: Option<Span> },
    /// A bare action name with no argument list to disambiguate an overload by,
    /// e.g., in hide.
    ActionSet {
        name: String,
        /// Every declaration sharing this name with a real span, in declaration order.
        declarations: Vec<Span>,
    },
    /// A PBES/PRES propositional-variable instantiation (`X(e1, e2)`), pushed at the
    /// identifier's own span rather than the whole `PropVarInst`.
    PropositionalVariable { name: String, declaration: Option<Span> },
    /// A state-formula fixpoint-variable instantiation (`X(e1, e2)` referencing an enclosing
    /// `mu X(...)`/`nu X(...)`). `declaration` is the enclosing binder's own span.
    ///
    /// Pushed at the whole occurrence's span: unlike `PropVarInst`, whose `identifier` field
    /// gives [`ResolvedName::PropositionalVariable`] a narrower span, `StateFrmKind::Id` carries
    /// none to use.
    StateVariable { name: String, declaration: Option<Span> },
    /// A sort-name reference, e.g., `D` in `map f: D -> D;`, anywhere where the
    /// user writes a sort by name.
    Sort { name: String, declaration: Option<Span> },
}

impl TypingInfo {
    /// All nodes, in generation order.
    pub fn nodes(&self) -> &[TypedNode] {
        &self.nodes
    }

    /// Consumes `self`, returning the nodes.
    pub fn into_nodes(self) -> Vec<TypedNode> {
        self.nodes
    }

    /// The most specific node whose span contains `offset`, for the usual
    /// hover/go-to-definition query.
    ///
    /// A span's end is treated as inclusive for this lookup, so a cursor
    /// sitting right after a token's last character still resolves to the
    /// token, matching the editor convention.
    ///
    /// When several nodes tie for the smallest span the last one in generation
    /// order is chosen. This can happen for synthesized nodes.
    pub fn at_offset(&self, offset: usize) -> Option<&TypedNode> {
        let mut best: Option<&TypedNode> = None;
        for node in &self.nodes {
            if node.span.start > offset || offset > node.span.end {
                continue;
            }

            let width = node.span.end - node.span.start;
            let is_narrower_or_tied = match best {
                Some(current) => width <= current.span.end - current.span.start,
                None => true,
            };

            if is_narrower_or_tied {
                best = Some(node);
            }
        }
        best
    }

    pub(crate) fn merge(&mut self, mut other: TypingInfo) {
        self.nodes.append(&mut other.nodes);
    }

    /// Records a single resolved name at `span` directly, for specification
    /// parts that are not data expressions.
    pub(crate) fn push(&mut self, span: Span, name: ResolvedName) {
        self.nodes.push(TypedNode {
            span,
            sort: None,
            name: Some(name),
        });
    }

    /// As [`Self::push`], but also records a sort, used for binders.
    pub(crate) fn push_typed(&mut self, span: Span, sort: Option<SortExpression>, name: ResolvedName) {
        self.nodes.push(TypedNode {
            span,
            sort,
            name: Some(name),
        });
    }
}

/// Builds the [`TypingInfo`] for `typing`, the already-computed Phase-3 result of one equation or
/// standalone expression. `typing.spans`/`typing.identifier_names` must be filled — true for
/// every `EquationTyping` this crate ever hands to a public caller, since both are only ever
/// omitted for `EquationRole::System`, which never reaches here.
pub(crate) fn build(spec: &DataSpecification, typing: &EquationTyping, variable_spans: &VariableSpans) -> TypingInfo {
    debug_assert_eq!(
        typing.spans.len(),
        typing.sorts.len(),
        "typing_info::build requires an EquationTyping built for EquationRole::User"
    );

    let index = DeclarationIndex::build(spec);
    let ctx = spec.context();
    let user_spec = spec.data_specification();

    let nodes = typing
        .spans
        .iter()
        .zip(&typing.sorts)
        .enumerate()
        .map(|(i, (span, &sort))| {
            let id = ExprId::new(i);
            let name = typing.names.get(&id).map(|&target| {
                let name = typing
                    .identifier_names
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| unreachable!("every named node has a recorded identifier"));
                let declaration = typing.declarations.get(&id).cloned();
                resolved_name(&index, variable_spans, target, name, declaration)
            });
            TypedNode {
                span: span.clone(),
                sort: Some(sort_expression(ctx, user_spec, sort)),
                name,
            }
        })
        .collect();

    TypingInfo { nodes }
}

fn resolved_name(
    index: &DeclarationIndex<'_>,
    variable_spans: &VariableSpans,
    target: NameTarget,
    name: String,
    declaration: Option<VarId>,
) -> ResolvedName {
    match target {
        NameTarget::Variable => {
            let declaration = declaration.and_then(|var_id| {
                let span = variable_spans.get(&var_id).cloned();
                debug_assert!(
                    span.is_some(),
                    "VariableSpans has no entry for {var_id:?} ({name:?}), which resolved as \
                     NameTarget::Variable — the variable-resolution pre-pass that builds both \
                     tables has gone out of sync"
                );
                span
            });
            ResolvedName::Variable { name, declaration }
        }
        NameTarget::Builtin => ResolvedName::Builtin { name },
        NameTarget::Op { sort } => {
            let constructor = index.constructors.get(&(name.as_str(), sort)).cloned();
            let mapping = index.mappings.get(&(name.as_str(), sort)).cloned();

            if let Some((id, declaration)) = constructor {
                ResolvedName::Constructor { name, id, declaration }
            } else if let Some((id, declaration)) = mapping {
                ResolvedName::Mapping { name, id, declaration }
            } else {
                // Declared only on the system-defined specification: no ConstructorId/MapId of
                // the *user* spec names it, and the system spec's own ids aren't meaningful to an
                // outside caller — but its declaration span (real since Milestone 3, see
                // `docs/spec-includes.md`) is, so look it up in `TypeCheckContext::system_symbol_spans`.
                let declaration = index.system_symbol_spans.get(&(name.clone(), sort)).cloned();
                ResolvedName::SystemDefined { name, declaration }
            }
        }
    }
}

/// A `(name, resolved sort) -> declaration` reverse lookup, built once per [`build`] call from
/// the user specification's own declaration lists. `(name, ResolvedSortId)` uniquely identifies a
/// symbol; the residual case of two literally duplicated declarations (legal: `map f: Nat; f:
/// Nat;`) resolves to the first, via `HashMap::entry(..).or_insert(..)` below.
struct DeclarationIndex<'a> {
    constructors: HashMap<(&'a str, ResolvedSortId), (ConstructorId, Option<Span>)>,
    mappings: HashMap<(&'a str, ResolvedSortId), (MapId, Option<Span>)>,
    /// `(name, resolved sort) -> declaration span` for every system-defined constructor/mapping,
    /// borrowed from `TypeCheckContext::system_symbol_spans`.
    system_symbol_spans: &'a HashMap<(String, ResolvedSortId), Span>,
}

impl<'a> DeclarationIndex<'a> {
    fn build(spec: &'a DataSpecification) -> Self {
        let mut constructors = HashMap::new();
        for decl in &spec.data_specification().constructor_declarations {
            let Some(id) = decl.id else {
                // Every declaration is assigned an id during `from_untyped`; a `DataSpecification`
                // that exists at all has already been through it.
                continue;
            };
            let sort = spec.sort_of_constructor(id);
            let declaration = declared_span(&decl.identifier.span);
            constructors
                .entry((decl.identifier.as_str(), sort))
                .or_insert((id, declaration));
        }

        let mut mappings = HashMap::new();
        for decl in &spec.data_specification().map_declarations {
            let Some(id) = decl.id else {
                continue;
            };
            let sort = spec.sort_of_map(id);
            let declaration = declared_span(&decl.identifier.span);
            mappings
                .entry((decl.identifier.as_str(), sort))
                .or_insert((id, declaration));
        }

        DeclarationIndex {
            constructors,
            mappings,
            system_symbol_spans: &spec.context().system_symbol_spans,
        }
    }
}

/// `Span::default()` marks a declaration synthesized without a real source location, rather than a
/// real position at the start of the file; normalize it to `None` so a consumer doesn't render a
/// misleading declaration site.
pub(crate) fn declared_span(span: &Span) -> Option<Span> {
    (*span != Span::default()).then(|| span.clone())
}

/// A single sort-name occurrence gathered by [`collect_sort_name_references`].
pub(crate) struct SortReference {
    pub(crate) span: Span,
    pub(crate) name: String,
    pub(crate) complex: Option<ComplexSort>,
    /// The occurrence's own [`SortId`].
    pub(crate) id: Option<SortId>,
}

/// Gathers the raw material [`push_sort_references`] turns into [`ResolvedName::Sort`] nodes.
/// Occurrences inside the data specification's own declarations must be collected before
/// [`crate::normalize_sorts`] rewrites the tree in place — an alias reference is replaced by its
/// own expansion, discarding both the reference's span and its name (see [`TypingInfo`]'s doc
/// comment on why a resolved sort's span can't be trusted). Occurrences inside a process/PBES/PRES
/// specification live in a syntax tree that pass never touches, so they can be gathered any time
/// after parsing instead.
///
/// Appends every `Reference`/`Resolved`/`Simple`/`Complex` leaf reachable in `sort` to `out`, as
/// `(its own occurrence span, name)`. Other compound kinds (`Product`, `Function`,
/// `FlattenedFunction`, `Struct`) are walked via [`Traverse`] until a named leaf is reached.
pub(crate) fn collect_sort_name_references(sort: &SortExpression, out: &mut Vec<SortReference>) {
    sort.visit::<Infallible, _>(|node| {
        match &node.node {
            SortExpressionKind::Reference(name) => {
                out.push(SortReference {
                    span: node.span.clone(),
                    name: name.clone(),
                    complex: None,
                    id: None,
                });
            }
            SortExpressionKind::Resolved(name, id) => {
                out.push(SortReference {
                    span: node.span.clone(),
                    name: name.clone(),
                    complex: None,
                    id: Some(*id),
                });
            }
            SortExpressionKind::Simple(sort) => {
                out.push(SortReference {
                    span: node.span.clone(),
                    name: sort.to_string(),
                    complex: None,
                    id: None,
                });
            }
            SortExpressionKind::Complex(complex_sort, _) => {
                let keyword = complex_sort.to_string();
                let span = Span {
                    start: node.span.start,
                    end: node.span.start + keyword.len(),
                };
                out.push(SortReference {
                    span,
                    name: keyword,
                    complex: Some(*complex_sort),
                    id: None,
                });
            }
            _ => {}
        }
        ControlFlow::Continue(())
    });
}

/// Every sort-name reference reachable in `spec`'s own declarations.
///
/// Must be called before [`crate::normalize_sorts`].
pub(crate) fn collect_data_specification_sort_references(spec: &UntypedDataSpecification) -> Vec<SortReference> {
    let mut out = Vec::new();

    for expr in spec.sort_declarations.iter().filter_map(|decl| decl.expr.as_ref()) {
        collect_sort_name_references(expr, &mut out);
    }

    for decl in &spec.constructor_declarations {
        collect_sort_name_references(&decl.sort, &mut out);
    }

    for decl in &spec.map_declarations {
        collect_sort_name_references(&decl.sort, &mut out);
    }

    for eqn_spec in &spec.equation_declarations {
        for var in &eqn_spec.variables {
            collect_sort_name_references(&var.sort, &mut out);
        }

        for eqn in &eqn_spec.equations {
            collect_data_expr_sort_references(&eqn.lhs, &mut out);
            collect_data_expr_sort_references(&eqn.rhs, &mut out);
            if let Some(condition) = &eqn.condition {
                collect_data_expr_sort_references(condition, &mut out);
            }
        }
    }

    out
}

/// Every variable a `(EqnSpecId, EquationId)` typing can reference.
pub(crate) fn collect_equation_variable_declarations(eqn_spec: &EqnSpec) -> VariableSpans {
    let mut spans = VariableSpans::default();
    for var in &eqn_spec.node.variables {
        let var_id = var
            .var_id
            .expect("resolve_data_specification_variables ran before typing_info");
        spans.insert(var_id, var.identifier.span.clone());
    }
    for equation in &eqn_spec.node.equations {
        if let Some(condition) = &equation.condition {
            collect_data_expr_variable_declarations(condition, &mut spans);
        }
        collect_data_expr_variable_declarations(&equation.lhs, &mut spans);
        collect_data_expr_variable_declarations(&equation.rhs, &mut spans);
    }
    spans
}

/// Every `lambda`/quantifier/comprehension/`whr` binder's own [`VarId`] and declaring span inside
/// `expr`, inserted into `out` — the binders a checked `DataExpr` can introduce *itself*, as
/// opposed to a `sum`/`dist`/PBES-PRES-modal-quantifier binder declared *outside* it, which
/// `checking::Scope` already carries a span for.
pub(crate) fn collect_data_expr_variable_declarations(expr: &DataExpr, out: &mut VariableSpans) {
    expr.visit::<Infallible, _>(|node| {
        match &node.node {
            DataExprKind::Lambda { variables, .. } | DataExprKind::Quantifier { variables, .. } => {
                for var in variables {
                    let var_id = var.var_id.expect("resolve_data_expr_variables/... ran before checking");
                    out.insert(var_id, var.identifier.span.clone());
                }
            }
            DataExprKind::SetBagComp { variable, .. } => {
                let var_id = variable
                    .var_id
                    .expect("resolve_data_expr_variables/... ran before checking");
                out.insert(var_id, variable.identifier.span.clone());
            }
            DataExprKind::Whr { assignments, .. } => {
                for assignment in assignments {
                    let var_id = assignment
                        .id
                        .expect("resolve_data_expr_variables/... ran before checking");
                    out.insert(var_id, assignment.span.clone());
                }
            }
            _ => {}
        }
        ControlFlow::Continue(())
    });
}

/// Every `lambda`/quantifier/comprehension binder's declared sort inside `expr`, appended to
/// `out`. `Traverse` recurses into `expr`'s own `DataExpr` children for free; only the binder's
/// own `sort` (an `IdDecl`, a different node type) needs handling at each matching node.
fn collect_data_expr_sort_references(expr: &DataExpr, out: &mut Vec<SortReference>) {
    expr.visit::<Infallible, _>(|node| {
        match &node.node {
            DataExprKind::Lambda { variables, .. } | DataExprKind::Quantifier { variables, .. } => {
                for var in variables {
                    collect_sort_name_references(&var.sort, out);
                }
            }
            DataExprKind::SetBagComp { variable, .. } => collect_sort_name_references(&variable.sort, out),
            _ => {}
        }
        ControlFlow::Continue(())
    });
}

/// `reference`'s own [`SortId`] declaration, and whether it names a user sort or a system-internal
/// one. Returns the whole declaration rather than just its name, since [`push_sort_references`]
/// needs the declaration's span.
fn sort_declaration_by_id(spec: &UntypedDataSpecification, id: SortId) -> Option<(&SortDecl, bool)> {
    let decl = spec.sort_declarations.get(*id)?;
    Some((decl, is_system_generated_name(&decl.identifier)))
}

/// Resolves each occurrence in `references` to its declaration and pushes
/// [`ResolvedName::Sort`]/[`ResolvedName::SystemDefined`] into `typing`.
///
/// An occurrence with a real [`SortId`] resolves via [`sort_declaration_by_id`], either from its
/// own [`SortReference::id`] or, for an unresolved [`SortExpressionKind::Reference`], by name via
/// `name_to_id`. Two shapes carry no [`SortId`]: a container-sort keyword (`List`, `Set`, …),
/// which has no declaration site, and a primitive basic-sort name (`Bool`, `Nat`, …), which still
/// has a real declaration span in `system`'s own textual re-declaration of it (`sort Nat;` in
/// `nat.mcrl2`), found by name instead.
pub(crate) fn push_sort_references(spec: &DataSpecification, references: &[SortReference], typing: &mut TypingInfo) {
    if references.is_empty() {
        return;
    }

    let mut name_to_id: HashMap<&str, SortId> = HashMap::new();
    for (i, decl) in spec.data_specification().sort_declarations.iter().enumerate() {
        name_to_id.entry(decl.identifier.as_str()).or_insert(SortId::new(i));
    }

    for reference in references {
        let name = &reference.name;
        let id = reference.id.or_else(|| name_to_id.get(name.as_str()).copied());

        if let Some(id) = id
            && let Some((decl, is_system)) = sort_declaration_by_id(spec.data_specification(), id)
        {
            let declaration = declared_span(&decl.span);
            typing.push(
                reference.span.clone(),
                if is_system {
                    ResolvedName::SystemDefined {
                        name: name.clone(),
                        declaration,
                    }
                } else {
                    ResolvedName::Sort {
                        name: name.clone(),
                        declaration,
                    }
                },
            );
        } else if reference.complex.is_some() {
            // A container-sort keyword (`List`, `Set`, …) never has a `sort` declaration of its
            // own — see `SortReference`'s doc comment.
            typing.push(
                reference.span.clone(),
                ResolvedName::SystemDefined {
                    name: name.clone(),
                    declaration: None,
                },
            );
        } else if let Some(decl) = spec
            .system_defined_specification()
            .sort_declarations
            .iter()
            .find(|decl| decl.identifier == *name)
        {
            typing.push(
                reference.span.clone(),
                ResolvedName::SystemDefined {
                    name: name.clone(),
                    declaration: declared_span(&decl.span),
                },
            );
        }
    }
}

/// Records a binder's own declaration occurrence.
pub(crate) fn push_binder_declaration(
    data: &DataSpecification,
    typing: &mut TypingInfo,
    span: Span,
    name: String,
    sort: ResolvedSortId,
) {
    let sort = sort_expression(data.context(), data.data_specification(), sort);
    typing.push_typed(
        span.clone(),
        Some(sort),
        ResolvedName::Variable {
            name,
            declaration: Some(span),
        },
    );
}

/// Rebuilds `id` as a [`SortExpression`], so it can be displayed via its existing
/// [`std::fmt::Display`] impl and so a `Def` sort carries a `SortId` a consumer can use for sort
/// go-to-definition. Mirrors [`crate::lower_sort`]'s structural recursion (same crate, targeting
/// the binary aterm format instead of the AST's own sort type).
///
/// Every produced node gets [`Span::default`].
fn sort_expression(ctx: &TypeCheckContext, spec: &UntypedDataSpecification, id: ResolvedSortId) -> SortExpression {
    match ctx.sorts.get(id) {
        ResolvedSort::Unit => unreachable_not_a_value_sort("Unit"),
        ResolvedSort::Primitive(sort) => SortExpressionKind::Simple(*sort).into(),
        ResolvedSort::Container { op, subsort } => {
            SortExpressionKind::Complex(*op, Box::new(sort_expression(ctx, spec, *subsort))).into()
        }
        ResolvedSort::Function { domain, range } => SortExpressionKind::FlattenedFunction {
            domain: domain.iter().map(|&sort| sort_expression(ctx, spec, sort)).collect(),
            range: Box::new(sort_expression(ctx, spec, *range)),
        }
        .into(),
        ResolvedSort::Def(def) => {
            let name = ctx.sort_display_name(spec, *def).into_owned();
            SortExpressionKind::Resolved(name, *def).into()
        }
        ResolvedSort::Var(_) => unreachable_not_a_value_sort("Var"),
    }
}
