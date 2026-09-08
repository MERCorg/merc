//! Public typing and name-resolution information for a checked specification,
//! keyed by [`Span`] rather than [`crate::ExprId`]: `ExprId` is assigned over
//! the *lowered* expression tree and can contain nodes with no counterpart in
//! the original `DataExpr` a caller parsed, so it cannot be reconstructed
//! externally.
//!
//! This module this deliberately kept apart from the rest of the crate:
//! everything else here is the type checker proper (sort resolution, signature
//! building, inference, lowering), while [`TypingInfo`] is a read-only
//! projection of what that pipeline already decided, re-keyed by source
//! position instead of by whichever internal id each phase happens to use. A
//! caller outside this crate only ever needs
//! [`TypingInfo`]/[`TypedNode`]/[`ResolvedName`], never the machinery that
//! builds them.
//!
//! # Caveats
//!
//! - A node synthesized during lowering has no span of its own and inherits the
//!   span of the whole surface expression it was lowered from — so more than
//!   one [`TypedNode`] can share a span; see [`TypingInfo::at_offset`]'s
//!   tie-break rule.
//! - [`TypedNode::sort`] is *reconstructed* from the resolved sort the type
//!   checker interned, not read from a declaration. It always carries
//!   [`Span::default`]: a resolved sort has no reliable source location of its
//!   own — name resolution and alias normalization both discard or relocate a
//!   [`SortExpression`]'s original span. Do not read source positions out of
//!   it. A sort *reference* (as opposed to the inferred sort of a
//!   data-expression node) is a different thing entirely — see
//!   [`ResolvedName::Sort`] — and its own span is exactly the occurrence's own
//!   source position, since it's captured before normalization ever touches it.
//! - `TypedNode::sort` is `None` for a node with no data sort at all — an
//!   [`ResolvedName::Action`]/[`ResolvedName::Process`] occurrence
//!   (`ProcessExprKind::Action`/ `Id`'s own name span): mCRL2 gives an
//!   action/process reference no data-expression sort to report (its declared
//!   argument sorts are a *domain*, not a value sort) — or a
//!   [`ResolvedName::Sort`] occurrence, which isn't a data expression at all —
//!   unlike every other `TypedNode`, which is always built from a checked
//!   [`merc_syntax::DataExpr`] and so always has one.

use std::collections::HashMap;
use std::convert::Infallible;
use std::ops::ControlFlow;

use merc_syntax::ConstructorId;
use merc_syntax::DataExpr;
use merc_syntax::DataExprKind;
use merc_syntax::MapId;
use merc_syntax::SortExpression;
use merc_syntax::SortExpressionKind;
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
use crate::VariableSpans;

/// The typing of a document's data specification (or of one expression checked via
/// [`DataSpecification::typecheck_expression_with_typing`]): one [`TypedNode`] per checked
/// expression node, in generation order.
///
/// Built by [`DataSpecification::typing_info`], [`DataSpecification::equation_typing_info`], and
/// [`DataSpecification::typecheck_expression_with_typing`].
#[derive(Debug, Default, Clone)]
pub struct TypingInfo {
    nodes: Vec<TypedNode>,
}

/// The typing of a single expression node. See this module's doc comment for the caveats on
/// `span` and `sort`.
#[derive(Debug, Clone)]
pub struct TypedNode {
    /// The node's location in the original source.
    pub span: Span,
    /// The node's inferred sort, reconstructed as a [`SortExpression`] so it can be printed via
    /// its existing [`std::fmt::Display`] impl. `None` for a node with no data sort at all — see
    /// this module's doc comment.
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
        /// See [`ResolvedName::Constructor::declaration`]. Resolved from the occurrence's own
        /// [`merc_syntax::VarId`] via the [`VariableSpans`] map in scope where this node was
        /// built — never a raw `VarId` a caller would have no way to look up on its own.
        declaration: Option<Span>,
    },
    /// A user-declared constructor.
    Constructor {
        name: String,
        id: ConstructorId,
        /// The declaration's span, when it has a real one.
        declaration: Option<Span>,
    },
    /// A user-declared mapping.
    Mapping {
        name: String,
        id: MapId,
        /// See [`ResolvedName::Constructor::declaration`].
        declaration: Option<Span>,
    },
    /// A declared Appendix-B symbol (`succ`, `@c0`, …) — not a user declaration.
    SystemDefined {
        name: String,
        /// See [`ResolvedName::Constructor::declaration`]. A caller that type checked against a
        /// throwaway `SourceMap` (e.g. via [`crate::DataSpecification::from_untyped`], discarded
        /// on return.
        declaration: Option<Span>,
    },
    /// A polymorphic built-in (`==`, `!=`, `<`, `<=`, `>`, `>=`, `if`, `in`, `#`, `|>`, …), whose
    /// concrete meaning follows from the inferred argument sorts rather than one declaration.
    Builtin { name: String },
    /// A declared action, resolved from a `ProcessExprKind::Action` occurrence by
    /// `check_action_or_process` once argument-sort overload resolution settles on exactly one
    /// `act` candidate — unlike `Variable`/`Constructor`/`Mapping`, this is never produced by
    /// `build` itself (an action name isn't part of any `DataExpr`), only pushed directly via
    /// `TypingInfo::push`.
    Action {
        name: String,
        /// The winning `act` declaration's own span. `None` only for a declaration with no real
        /// span, which should not arise in practice — see [`ResolvedName::Constructor::declaration`].
        declaration: Option<Span>,
    },
    /// A declared process, resolved from a `ProcessExprKind::Action`/`Id` occurrence (a positional
    /// or assignment-form instantiation) the same way as [`ResolvedName::Action`], by
    /// `check_action_or_process`/`check_instantiation`.
    Process {
        name: String,
        /// See [`ResolvedName::Action::declaration`].
        declaration: Option<Span>,
    },
    /// A bare action name with no argument list to disambiguate an overload by — the
    /// `hide`/`block`/`allow`/`comm`/`rename` action-name sets.
    ActionSet {
        name: String,
        /// Every declaration sharing this name with a real span, in declaration order.
        declarations: Vec<Span>,
    },
    /// A PBES/PRES propositional-variable instantiation (`X(e1, e2)`.
    ///
    /// Pushed at the identifier's own span, not the whole `PropVarInst` (`X(e1, e2)`) — see
    /// [`ResolvedName::Action::declaration`]'s counterpart in `check_action_or_process`.
    PropositionalVariable {
        name: String,
        /// See [`ResolvedName::Action::declaration`].
        declaration: Option<Span>,
    },
    /// A state-formula fixpoint-variable instantiation (`X(e1, e2)` referencing an enclosing
    /// `mu X(...)`/`nu X(...)`).
    ///
    /// Pushed at the whole `Id` node's own span, not a separate identifier span —
    /// `StateFrmKind::Id` carries no span narrower than the whole `name`/`name(args)` occurrence,
    /// unlike [`ResolvedName::PropositionalVariable`]'s `PropVarInst`, which does. `declaration` is
    /// its enclosing `mu`/`nu`'s own `StateVarDecl` span, for the same reason.
    StateVariable {
        name: String,
        /// See [`ResolvedName::Action::declaration`].
        declaration: Option<Span>,
    },
    /// A sort-name reference: `D` in `map f: D -> D;`, on the right-hand side of a `sort E = D;`
    /// alias, a `struct`'s own field sort, a `List(D)`'s parameter, or a `sum`/`dist`/quantifier/
    /// `lambda`/comprehension binder's declared sort — anywhere a user writes a sort *by name*.
    ///
    /// Every built-in (`Bool`, `Nat`, `List`, …) parses straight to its own dedicated
    /// [`SortExpressionKind`] variant, never a named `Reference`/`Resolved` — so a
    /// [`ResolvedName::Sort`] node is only ever produced for a name that refers to a user's own
    /// `sort` declaration.
    ///
    /// Unlike [`ResolvedName::Constructor`]/[`ResolvedName::Mapping`], a sort name is never
    /// overloaded.
    ///
    /// Captured and pushed directly via `TypingInfo::push`, the same way as
    /// [`ResolvedName::Action`]/[`ResolvedName::Process`]/[`ResolvedName::PropositionalVariable`]:
    /// a sort name never appears inside a checked `DataExpr`, only in a declaration's own
    /// signature, so `build` itself never produces one. See
    /// `collect_sort_name_references`/`push_sort_references` for where every occurrence is
    /// gathered from, and [`crate::DataSpecification::from_untyped_with`] for *why* the
    /// data-specification half of that gathering has to run before alias normalization touches
    /// the tree.
    Sort {
        name: String,
        /// The declaring `sort` block's own span. `None` only for a declaration with no real
        /// span, which should not arise in practice — see [`ResolvedName::Constructor::declaration`].
        declaration: Option<Span>,
    },
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

    /// The most specific node whose span contains `offset` — the usual hover/go-to-definition
    /// query.
    ///
    /// A span's end is treated as inclusive for this lookup, so a cursor sitting right after a
    /// token's last character (offset == that token's `span.end`) still resolves to the token,
    /// matching the editor convention that a cursor between two characters belongs to the one on
    /// its left. This can put a node in a tie with a zero-gap neighbour starting exactly at that
    /// offset (e.g. the boundary between `1` and `+` in `1+1`); the usual tie-break below decides.
    ///
    /// When several nodes tie for the smallest span (a synthesized node sharing its surface
    /// expression's span with that expression itself — `x + y`'s synthesized `Id("+")` and its
    /// `Application` both span the whole expression), the *last* one in generation order wins,
    /// which is the more specific of the two in every case this arises: the operator name rather
    /// than the application's result.
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

    /// Records a single resolved name at `span` directly, with no backing `DataExpr`/sort — the
    /// only way a [`ResolvedName::Action`]/[`ResolvedName::Process`]/
    /// [`ResolvedName::PropositionalVariable`]/[`ResolvedName::Sort`] node is ever added, since
    /// [`build`] only ever sees the `DataExpr` half of a specification.
    pub(crate) fn push(&mut self, span: Span, name: ResolvedName) {
        self.nodes.push(TypedNode {
            span,
            sort: None,
            name: Some(name),
        });
    }

    /// As [`Self::push`], but also records a sort — used for a binder's own declaration
    /// occurrence.
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
    let system_spec = spec.system_defined_specification();

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
                sort: Some(sort_expression(ctx, user_spec, system_spec, sort)),
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
/// symbol (`compute_signature` rejects a `cons`/`map` conflict exactly when both the name and the
/// resolved sort coincide); the residual case of two *literally duplicated* declarations (legal:
/// `map f: Nat; f: Nat;`) resolves to the first, via `HashMap::entry(..).or_insert(..)` below.
struct DeclarationIndex<'a> {
    constructors: HashMap<(&'a str, ResolvedSortId), (ConstructorId, Option<Span>)>,
    mappings: HashMap<(&'a str, ResolvedSortId), (MapId, Option<Span>)>,
    /// `(name, resolved sort) -> declaration span` for every system-defined constructor/mapping —
    /// borrowed straight from `TypeCheckContext::system_symbol_spans` (already built as a side
    /// effect of signature resolution; see its doc comment for why that, and not
    /// `sort_of_constructor`/`sort_of_map`, is the right source for a system-defined id). Owned
    /// `String` keys, unlike `constructors`/`mappings` above, since that field's key type is fixed
    /// by where it's populated, not by this reader.
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

// ─── sort-name references (`ResolvedName::Sort`) ────────────────────────────
//
// Everything below gathers the raw material [`push_sort_references`] turns into
// [`ResolvedName::Sort`] nodes. Two-phase, because a sort-name occurrence can live in either of
// two places with very different lifetimes:
//
// - Inside the data specification's own declarations (`cons`/`map` signatures, a `var`-block, a
//   sort alias's own right-hand side, an equation's own `lambda`/quantifier/comprehension binder)
//   — [`collect_data_specification_sort_references`] gathers these, and *must* run before
//   [`crate::normalize_sorts`] rewrites the tree in place (an alias reference is replaced by its
//   own expansion, discarding both the reference's span and its name — see this module's caveat
//   on why a resolved sort's span can't be trusted). [`crate::DataSpecification::from_untyped_with`]
//   calls this at exactly the right point in its own pipeline and stashes the raw `(span, name)`
//   pairs on `self` for [`crate::DataSpecification::typing_info`] to resolve once the
//   specification (and its declaration spans, which normalization never touches) is final.
// - Inside a process/PBES/PRES specification's own declarations (`act`/`proc` signatures, `glob`
//   variables, every `sum`/`dist`/quantifier/`inf`/`sup` binder) — these live in a syntax tree
//   `DataSpecification::from_untyped_with` never touches at all, so they can be gathered any time
//   after parsing; `crate::checking::collect_binder_sorts` and each of
//   `process`/`pbes`/`pres`'s own `check_*_specification` do so directly, once the whole
//   specification (and so its final declaration spans) is available.

/// Every `Reference`/`Resolved`/`Simple` leaf reachable in `sort`, appended to `out` as `(its own
/// occurrence span, name)`. `sort`'s compound kinds (`Product`, `Function`, `FlattenedFunction`,
/// `Complex`, `Struct`) are walked via [`Traverse`] until a named leaf is reached.
///
/// `Simple` (`Bool`, `Nat`, `Pos`, `Int`, `Real`) resolves through [`push_sort_references`]'s
/// system-defined fallback, never through a user's own `sort_declarations` — each names its own
/// top-level `sort` declaration in the corresponding `crates/syntax/spec/*.mcrl2` template (e.g.
/// `sort Nat;` in `nat.mcrl2`), always present in `system_defined_specification()` regardless of
/// which basic sorts a specification actually uses. `Complex` (`List`, `Set`, …) has no such
/// declaration in its own template (a container's grammar production needs no `sort` line the way
/// a `Simple` one does) and so still has nothing to report here — seeing its own name resolve
/// silently (see [`push_sort_references`]) rather than being collected as a dead end.
pub(crate) fn collect_sort_name_references(sort: &SortExpression, out: &mut Vec<(Span, String)>) {
    sort.visit::<Infallible, _>(|node| {
        match &node.node {
            SortExpressionKind::Reference(name) | SortExpressionKind::Resolved(name, _) => {
                out.push((node.span.clone(), name.clone()));
            }
            SortExpressionKind::Simple(sort) => {
                out.push((node.span.clone(), sort.to_string()));
            }
            _ => {}
        }
        ControlFlow::Continue(())
    });
}

/// Every sort-name reference reachable in `spec`'s own declarations: `cons`/`map` signatures, a
/// `var`-block declaration, a sort alias's own right-hand side (including a `struct`'s field
/// sorts — already flattened into fresh `cons`/`map` declarations by the time this runs, see
/// [`crate::desugar_structured_sorts`], so no separate `Struct` case is needed here), and a
/// `lambda`/quantifier/comprehension binder inside an equation. Does *not* cover
/// `act`/`proc`/`glob`/`sum`/`dist`/PBES-or-PRES-binder sorts — see this section's own doc
/// comment for where those are gathered instead.
///
/// Must be called before [`crate::normalize_sorts`] — see this section's doc comment.
pub(crate) fn collect_data_specification_sort_references(spec: &UntypedDataSpecification) -> Vec<(Span, String)> {
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

/// Every `lambda`/quantifier/comprehension binder's declared sort inside `expr`, appended to
/// `out`. `Traverse` recurses into `expr`'s own `DataExpr` children for free; only the binder's
/// own `sort` (an `IdDecl`, a different node type) needs handling at each matching node — mirrors
/// `docs/name_resolution.md`'s pending TODO to consolidate this with
/// `checking::collect_binder_sorts`, the equivalent walk for a `sum`/`dist`/quantifier binder
/// *outside* the data specification.
fn collect_data_expr_sort_references(expr: &DataExpr, out: &mut Vec<(Span, String)>) {
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

/// Resolves each `(occurrence span, sort name)` pair in `references` against `spec`'s own sort
/// declarations first, falling back to `system_defined_specification()`'s (a `Simple` built-in
/// like `Nat` never matches the former, only the latter — see [`collect_sort_name_references`]),
/// pushing [`ResolvedName::Sort`]/[`ResolvedName::SystemDefined`] respectively into `typing` for
/// each. A name matching neither is silently skipped, never pushed with `declaration: None`: a
/// name genuinely missing from both means the specification didn't actually type check (this
/// function's callers only ever run once it did) — see `declared_span`'s own doc comment for the
/// one legitimate `None` case, a synthesized declaration with no real source location, which is
/// still pushed (just with a `None` declaration) rather than treated as missing.
///
/// A sort name is never overloaded (mCRL2 has one flat sort namespace), so — unlike
/// [`DeclarationIndex`] — this only needs two plain `name -> declaration span` maps, built fresh
/// per call; a caller pushing many references in one batch (every entry point today does) still
/// pays for it only once.
pub(crate) fn push_sort_references(spec: &DataSpecification, references: &[(Span, String)], typing: &mut TypingInfo) {
    if references.is_empty() {
        return;
    }

    let declared: HashMap<&str, Option<Span>> = spec
        .data_specification()
        .sort_declarations
        .iter()
        .map(|decl| (decl.identifier.as_str(), declared_span(&decl.span)))
        .collect();
    let system_declared: HashMap<&str, Option<Span>> = spec
        .system_defined_specification()
        .sort_declarations
        .iter()
        .map(|decl| (decl.identifier.as_str(), declared_span(&decl.span)))
        .collect();

    for (span, name) in references {
        if let Some(declaration) = declared.get(name.as_str()).cloned() {
            typing.push(
                span.clone(),
                ResolvedName::Sort {
                    name: name.clone(),
                    declaration,
                },
            );
        } else if let Some(declaration) = system_declared.get(name.as_str()).cloned() {
            typing.push(
                span.clone(),
                ResolvedName::SystemDefined {
                    name: name.clone(),
                    declaration,
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
    let sort = sort_expression(
        data.context(),
        data.data_specification(),
        data.system_defined_specification(),
        sort,
    );
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
/// [`std::fmt::Display`] impl and so a `Def` sort carries a `DefId` a consumer can use for sort
/// go-to-definition. Mirrors [`crate::lower_sort`]'s structural recursion (same crate, targeting
/// the binary aterm format instead of the AST's own sort type).
///
/// Every produced node gets [`Span::default`].
fn sort_expression(
    ctx: &TypeCheckContext,
    spec: &UntypedDataSpecification,
    system: &UntypedDataSpecification,
    id: ResolvedSortId,
) -> SortExpression {
    match ctx.sorts.get(id) {
        ResolvedSort::Unit => unreachable!(
            "Unit is only used for the sort of an action, never a data-expression sort, and \
             sort_expression only ever renders the sort of a data expression"
        ),
        ResolvedSort::Primitive(sort) => SortExpressionKind::Simple(*sort).into(),
        ResolvedSort::Generic { op, subsort } => {
            SortExpressionKind::Complex(*op, Box::new(sort_expression(ctx, spec, system, *subsort))).into()
        }
        ResolvedSort::Function { domain, range } => SortExpressionKind::FlattenedFunction {
            domain: domain
                .iter()
                .map(|&sort| sort_expression(ctx, spec, system, sort))
                .collect(),
            range: Box::new(sort_expression(ctx, spec, system, *range)),
        }
        .into(),
        ResolvedSort::Def(def) => {
            let name = ctx
                .sort_name(spec, system, *def)
                .map(str::to_string)
                .unwrap_or_else(|| format!("@sort_{}", def.value()));
            SortExpressionKind::Resolved(name, *def).into()
        }
        ResolvedSort::Var(_) => unreachable!(
            "a bound type variable is always instantiated to a fresh unification variable before \
             Phase-3 solving produces a node's final ResolvedSortId, so sort_expression never renders one"
        ),
    }
}
