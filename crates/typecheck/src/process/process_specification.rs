//! [`ProcessSpecification`]: type checking for a whole `UntypedProcessSpecification` — the data
//! specification (delegated to [`DataSpecification`]) plus its actions, processes, and `init`.

use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::ControlFlow;

use merc_syntax::ActDecl;
use merc_syntax::IdDecl;
use merc_syntax::ProcDecl;
use merc_syntax::ProcessExpr;
use merc_syntax::SortExpression;
use merc_syntax::SortExpressionKind;
use merc_syntax::SourceMap;
use merc_syntax::Span;
use merc_syntax::Traverse;
use merc_syntax::UntypedProcessSpecification;

use crate::DataSpecification;
use crate::NumberEncoding;
use crate::ResolvedSortId;
use crate::TypingInfo;

use super::ProcessError;
use super::check;
use super::disambiguation;

/// A type-checked mCRL2 process specification: the data specification plus its `act`, `proc`,
/// `glob`, and `init` declarations, all resolved and checked against it.
///
/// See the crate README for what's scoped in and out of this.
pub struct ProcessSpecification {
    /// The original specification, *minus* its data specification.
    spec: UntypedProcessSpecification,
    data: DataSpecification,
    /// Every process-body expression's `TypingInfo`, merged during the construction walk (see
    /// [`Self::typing_info`]).
    typing: TypingInfo,
}

impl ProcessSpecification {
    /// Type checks `spec` against a fresh, throwaway [`SourceMap`], using the default number
    /// encoding. See [`Self::from_untyped_with`].
    ///
    /// Prefer [`Self::from_untyped_with`] with a real `sources`.
    pub fn from_untyped(spec: UntypedProcessSpecification) -> Result<Self, ProcessError> {
        Self::from_untyped_with(spec, NumberEncoding::default(), &mut SourceMap::new())
    }

    /// Type checks `spec`: its data specification first (exactly as
    /// [`DataSpecification::from_untyped_with`] does), then its action declarations' argument
    /// sorts, its global variables, and every `proc` body and `init` against them.
    ///
    /// `sources` accumulates the system-defined ("Appendix B") content this generates, the same
    /// way [`DataSpecification::from_untyped_with`]'s own `sources` parameter does — pass the
    /// `SourceMap` `spec` was parsed (and, if applicable, `%import`-resolved) against so every
    /// span, whether from `spec`'s own text, something it imports, or Appendix B, renders
    /// correctly against one shared offset space; pass a fresh one if nothing else needs to share
    /// it.
    pub fn from_untyped_with(
        mut spec: UntypedProcessSpecification,
        encoding: NumberEncoding,
        sources: &mut SourceMap,
    ) -> Result<Self, ProcessError> {
        // Semantic-aware disambiguation first.
        disambiguation::disambiguate_process_specification(&mut spec);

        // A pure syntactic pass, before anything else needs `spec` — see
        // `resolution::variable_resolution`.
        crate::resolve_process_variables(&mut spec);

        let data_spec = std::mem::take(&mut spec.data_specification);
        let mut data = DataSpecification::from_untyped_with(data_spec, encoding, sources)?;

        let tables = DeclarationTables::build(&mut data, &spec)?;
        let typing = check::check_process_specification(&mut data, &tables, &spec)?;

        Ok(ProcessSpecification { spec, data, typing })
    }

    /// The checked data specification.
    pub fn data_specification(&self) -> &DataSpecification {
        &self.data
    }

    /// Consumes `self`, returning the checked data specification.
    pub fn into_data_specification(self) -> DataSpecification {
        self.data
    }

    /// The `glob` declarations, in scope in every process body and in `init`.
    pub fn global_variables(&self) -> &[IdDecl] {
        &self.spec.global_variables
    }

    /// The `act` declarations.
    pub fn action_declarations(&self) -> &[ActDecl] {
        &self.spec.action_declarations
    }

    /// The `proc` declarations.
    pub fn process_declarations(&self) -> &[ProcDecl] {
        &self.spec.process_declarations
    }

    /// The `init` process, if the specification declares one. Absent is not itself an error: a
    /// library-only specification with no `init` is legitimate.
    pub fn init(&self) -> Option<&ProcessExpr> {
        self.spec.init.as_ref()
    }

    /// Every checked expression's typing across the *whole* specification — every `eqn` (via
    /// [`DataSpecification::typing_info`]) plus every process-body expression (action arguments,
    /// process-instantiation arguments, conditions, time bounds, `dist` weights), span-keyed so
    /// hover/go-to-definition/inlay-hints can look up a sub-expression by source position anywhere
    /// in the document (see [`TypingInfo::at_offset`]) without caring which half of the grammar it
    /// came from.
    ///
    /// The process-body half needs no separate memoization: it was already computed once, during
    /// the construction walk `from_untyped_with` runs anyway, and is just cloned out of the stored
    /// value here. The `eqn` half *is* memoized, one level down — see
    /// [`DataSpecification::typing_info`] — so a repeated call is cheap either way.
    pub fn typing_info(&mut self) -> TypingInfo {
        let mut info = self.data.typing_info();
        info.merge(self.typing.clone());
        info
    }
}

/// The resolved `act`/`proc` declaration tables, built once by [`Self::build`] and used by
/// [`super::check`]'s scoped walk to resolve every `Action`/`Id` process-expression node it
/// reaches, and by [`super::process_specification`] to check each declaration's own body against
/// its own parameters.
pub(super) struct DeclarationTables {
    /// Resolved sort of each `glob` declaration, parallel to `spec.global_variables`.
    pub(super) global_sorts: Vec<ResolvedSortId>,
    /// Resolved `(name, sort)` parameters of each process declaration, parallel to
    /// `spec.process_declarations`.
    pub(super) process_params: Vec<Vec<(String, ResolvedSortId)>>,
    /// `spec.process_declarations[i].identifier.span`, parallel to
    /// `process_params`.
    pub(super) process_decl_spans: Vec<Span>,
    /// name -> indices into `spec.process_declarations`/`process_params` declaring it.
    pub(super) processes_by_name: HashMap<String, Vec<usize>>,
    /// Resolved argument-sort domain of each action declaration, parallel to
    /// `spec.action_declarations`.
    pub(super) action_domains: Vec<Vec<ResolvedSortId>>,
    /// `spec.action_declarations[i].identifier.span`, parallel to `action_domains` — see
    /// `process_decl_spans`.
    pub(super) action_decl_spans: Vec<Span>,
    /// name -> indices into `spec.action_declarations`/`action_domains` declaring it.
    pub(super) actions_by_name: HashMap<String, Vec<usize>>,
}

impl DeclarationTables {
    fn build(data: &mut DataSpecification, spec: &UntypedProcessSpecification) -> Result<Self, ProcessError> {
        let mut action_domains = Vec::with_capacity(spec.action_declarations.len());
        let mut action_decl_spans = Vec::with_capacity(spec.action_declarations.len());
        let mut actions_by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, decl) in spec.action_declarations.iter().enumerate() {
            let domain = decl
                .args
                .iter()
                .map(|sort| resolve_declared_sort(data, sort))
                .collect::<Result<Vec<_>, _>>()?;
            actions_by_name
                .entry(decl.identifier.node.clone())
                .or_default()
                .push(index);
            action_domains.push(domain);
            action_decl_spans.push(decl.identifier.span.clone());
        }

        let mut process_params = Vec::with_capacity(spec.process_declarations.len());
        let mut process_decl_spans = Vec::with_capacity(spec.process_declarations.len());
        let mut processes_by_name: HashMap<String, Vec<usize>> = HashMap::new();
        for (index, decl) in spec.process_declarations.iter().enumerate() {
            let mut params = Vec::with_capacity(decl.params.len());
            let mut seen = HashSet::new();
            for param in &decl.params {
                if !seen.insert(param.identifier.as_str()) {
                    return Err(ProcessError::DuplicateProcessParameter {
                        process: decl.identifier.node.clone(),
                        name: param.identifier.node.clone(),
                        span: param.identifier.span.clone(),
                    });
                }
                let sort = resolve_declared_sort(data, &param.sort)?;
                params.push((param.identifier.node.clone(), sort));
            }
            processes_by_name
                .entry(decl.identifier.node.clone())
                .or_default()
                .push(index);
            process_params.push(params);
            process_decl_spans.push(decl.identifier.span.clone());
        }

        // A name declared as both an action and a process would make `Action(name, args)`
        // (used for both an action instance and a positional process instantiation, see the crate
        // README) permanently ambiguous between the two tables. Iterated in source order, not via
        // `actions_by_name`'s (unordered) keys, so which conflicting name gets reported is
        // deterministic.
        for decl in &spec.action_declarations {
            if processes_by_name.contains_key(&decl.identifier.node) {
                return Err(ProcessError::ActionAndProcessConflict {
                    name: decl.identifier.node.clone(),
                    span: decl.identifier.span.clone(),
                });
            }
        }

        let mut global_sorts = Vec::with_capacity(spec.global_variables.len());
        let mut seen_globals = HashSet::new();
        for decl in &spec.global_variables {
            if !seen_globals.insert(decl.identifier.as_str()) {
                return Err(ProcessError::DuplicateGlobalVariable {
                    name: decl.identifier.node.clone(),
                    span: decl.identifier.span.clone(),
                });
            }
            global_sorts.push(resolve_declared_sort(data, &decl.sort)?);
        }

        Ok(DeclarationTables {
            global_sorts,
            process_params,
            process_decl_spans,
            processes_by_name,
            action_domains,
            action_decl_spans,
            actions_by_name,
        })
    }
}

/// Resolves a sort expression occurring in an `act`/`proc`/`glob` declaration: rejects an
/// anonymous `struct` (never legal here), then defers to
/// [`DataSpecification::resolve_declared_sort`] for the rest.
pub(super) fn resolve_declared_sort(
    data: &mut DataSpecification,
    sort: &SortExpression,
) -> Result<ResolvedSortId, ProcessError> {
    if let Some(span) = find_anonymous_struct(sort) {
        return Err(ProcessError::AnonymousStructInDeclaration { span });
    }
    Ok(data.resolve_declared_sort(sort)?)
}

/// The span of the first anonymous `struct` anywhere within `sort`, if any.
fn find_anonymous_struct(sort: &SortExpression) -> Option<Span> {
    sort.visit(|expr| match &expr.node {
        SortExpressionKind::Struct { .. } => ControlFlow::Break(expr.span.clone()),
        _ => ControlFlow::Continue(()),
    })
}
