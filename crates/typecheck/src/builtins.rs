use std::sync::LazyLock;

use merc_syntax::Sort;
use merc_syntax::UntypedDataSpecification;

use crate::parse_rigid_template;

/// The five built-in basic sorts. They are always present in a specification.
const BASIC_SORTS: [Sort; 5] = [Sort::Bool, Sort::Pos, Sort::Nat, Sort::Int, Sort::Real];

/// Whether `name` names one of the [`BASIC_SORTS`].
pub(crate) fn is_basic_sort_name(name: &str) -> bool {
    BASIC_SORTS.iter().any(|sort| sort.name() == name)
}

/// Whether `name` uses the reserved `@`-prefix convention every system-generated
/// constructor/mapping/sort declaration (`@c0`, `@cPair`, `@zero_`, `@NatPair`, …) uses, as opposed
/// to a user's own declaration.
pub(crate) fn is_system_generated_name(name: &str) -> bool {
    name.starts_with('@')
}

/// [BUILTIN_SCHEME_TEMPLATE]'s source text.
pub(crate) const BUILTIN_SCHEME_TEMPLATE_TEXT: &str = include_str!("../../syntax/spec/comparison.mcrl2");

/// The polymorphic built-in operators that exist for *every* sort: the
/// comparison operators and the conditional `if`.
pub(crate) static BUILTIN_SCHEME_TEMPLATE: LazyLock<UntypedDataSpecification> =
    LazyLock::new(|| parse_rigid_template(BUILTIN_SCHEME_TEMPLATE_TEXT));

/// The names of the polymorphic built-in schemes, derived from
/// [`BUILTIN_SCHEME_TEMPLATE`] so the list has a single definition. These names
/// are usable without a declaration, so the well-formedness and reserved-name
/// checks admit them.
pub(crate) fn builtin_scheme_names() -> impl Iterator<Item = &'static str> {
    BUILTIN_SCHEME_TEMPLATE
        .map_declarations
        .iter()
        .map(|decl| decl.identifier.as_str())
}
