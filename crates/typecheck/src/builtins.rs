use std::sync::LazyLock;

use merc_syntax::UntypedDataSpecification;

use crate::parse_rigid_template;

/// The five built-in basic sorts. They are always present in a specification,
/// resolve to primitives, and may not receive user constructors. Public so a
/// caller that needs to recognize these names without a full type-checking pass (e.g. an LSP's
/// syntax highlighting) has a single source of truth instead of a hand-copied list of its own.
pub const BASIC_SORT_NAMES: [&str; 5] = ["Bool", "Pos", "Nat", "Int", "Real"];

/// Whether `name` is one of the [`BASIC_SORT_NAMES`].
pub(crate) fn is_basic_sort_name(name: &str) -> bool {
    BASIC_SORT_NAMES.contains(&name)
}

/// [BUILTIN_SCHEME_TEMPLATE]'s source text.
pub(crate) const BUILTIN_SCHEME_TEMPLATE_TEXT: &str = "type_var S; \
     map ==: S # S -> Bool; !=: S # S -> Bool; \
     <: S # S -> Bool; <=: S # S -> Bool; >: S # S -> Bool; >=: S # S -> Bool; \
     if: Bool # S # S -> S; \
     var x, y: S; \
     eqn x == x = true; \
         x != y = !(x == y); \
         x < x = false; \
         x <= x = true; \
         x > y = y < x; \
         x >= y = y <= x; \
         if(true, x, y) = x; \
         if(false, x, y) = y;";

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
