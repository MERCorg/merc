// Monomorphizes system-defined (Appendix-B) equations for lowering: turns each container/
// function-update/comparison template instantiation the specification actually uses into ground
// content by substituting into the template's own already-proven, rigid typing — never by
// inferring it again. See `TemplateInstantiation`'s own doc comment for why substitution is what
// makes two instantiations of the same template (`Bag(Nat)`, `Bag(D)`) sound without re-checking
// either one, and `crate::check_system_equations`'s doc comment for the one kind of system
// equation this does *not* cover (`system`'s own basics/desugared-struct equations, which are
// genuinely inferred, once).

use std::collections::HashMap;
use std::sync::Arc;

use merc_syntax::TypeVarId;
use merc_syntax::UntypedDataSpecification;

use crate::EquationTyping;
use crate::NameTarget;
use crate::ResolvedSortId;
use crate::TemplateInstantiation;
use crate::TypeCheckContext;
use crate::resolve_sort;

/// Specializes a template's own proven [`EquationTyping`] into the typing of one concrete
/// instantiation, substituting `substitution[i]` for `vars[i]` (the template's own `type_var`
/// declarations, in declaration order) throughout every sort the typing recorded.
///
/// A lowering concern, not an inference one: nothing here can fail or discover anything
/// `check_template_equations` didn't already prove about the template itself.
pub(crate) fn specialize_template_typing(
    ctx: &mut TypeCheckContext,
    typing: &EquationTyping,
    vars: &[TypeVarId],
    substitution: &[ResolvedSortId],
) -> EquationTyping {
    debug_assert_eq!(
        vars.len(),
        substitution.len(),
        "one concrete sort per template type variable"
    );
    let substitute = |ctx: &mut TypeCheckContext, sort: ResolvedSortId| {
        vars.iter()
            .zip(substitution)
            .fold(sort, |sort, (&var, &with)| ctx.sorts.substitute_var(sort, var, with))
    };

    let sorts = typing.sorts.iter().map(|&sort| substitute(ctx, sort)).collect();
    let names = typing
        .names
        .iter()
        .map(|(&id, target)| {
            let target = match *target {
                NameTarget::Op { sort } => NameTarget::Op {
                    sort: substitute(ctx, sort),
                },
                other => other,
            };
            (id, target)
        })
        .collect();

    EquationTyping {
        sorts,
        spans: Vec::new(),
        names,
        identifier_names: HashMap::new(),
        declarations: HashMap::new(),
        node_ids: HashMap::new(),
    }
}

/// Populates `ctx.system_equation_typing` for every equation of `generated`.
///
/// `generated` is assembled *entirely* from template instantiations:
/// `build_system_defined_specification`/`extend_system_with_inferred_sorts` never append an
/// equation block to it except through `merge_generated`, which records one
/// [`TemplateInstantiation`] for every block it appends. So every block has a proven, rigid
/// template typing (`ctx.template_typings`, populated once up front by
/// `check_container_templates`/`check_comparison_template`/
/// `check_multi_argument_function_update_template` during `DataSpecification::from_untyped_with`)
/// to specialize from, and this function never calls `infer_equation` — only
/// [`specialize_template_typing`]'s substitution. The `debug_assert_eq!` below pins that coverage
/// down as a checked property rather than a comment.
pub(crate) fn instantiate_system_equations(
    ctx: &mut TypeCheckContext,
    user_spec: &UntypedDataSpecification,
    generated: &UntypedDataSpecification,
    instantiations: &[TemplateInstantiation],
) {
    debug_assert_eq!(
        instantiations
            .iter()
            .map(|instantiation| instantiation.equation_range.len())
            .sum::<usize>(),
        generated.equation_declarations.len(),
        "every generated equation block must come from exactly one template instantiation"
    );

    for instantiation in instantiations {
        let substitution: Vec<ResolvedSortId> = instantiation
            .substitution
            .iter()
            // Drawn from the user's own already-resolved sort tree, so `resolve_sort` resolves
            // every entry infallibly.
            .map(|sort| resolve_sort(ctx, user_spec, sort))
            .collect();

        for (local_index, block_index) in instantiation.equation_range.clone().enumerate() {
            instantiate_equation_block(ctx, generated, instantiation, &substitution, block_index, local_index);
        }
    }
}

/// Specializes one `generated.equation_declarations[block_index]` block, the local `local_index`-th
/// one `instantiation`'s own template contributes.
fn instantiate_equation_block(
    ctx: &mut TypeCheckContext,
    generated: &UntypedDataSpecification,
    instantiation: &TemplateInstantiation,
    substitution: &[ResolvedSortId],
    block_index: usize,
    local_index: usize,
) {
    let eqn_spec = &generated.equation_declarations[block_index];
    let eqn_spec_id = eqn_spec
        .id
        .expect("assign_declaration_ids ran on the generated content before instantiate_system_equations");

    let check = ctx.template_typings.get(&instantiation.template).unwrap_or_else(|| {
        panic!(
            "template '{}' was not checked before instantiation — every template is proven once, \
             up front, by `DataSpecification::from_untyped_with`",
            instantiation.template
        )
    });
    let type_vars = check.type_vars.clone();
    let block_typings = check.typings[local_index].clone();

    for (equation, template_typing) in eqn_spec.equations.iter().zip(&block_typings) {
        let equation_id = equation
            .id
            .expect("assign_declaration_ids ran on the generated content before instantiate_system_equations");
        let specialized = specialize_template_typing(ctx, template_typing, &type_vars, substitution);
        ctx.system_equation_typing
            .insert((eqn_spec_id, equation_id), Ok(Arc::new(specialized)));
    }
}
