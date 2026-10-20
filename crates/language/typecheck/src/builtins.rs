use merc_syntax::Sort;

/// The five built-in basic sorts. They are always present in a specification.
const BASIC_SORTS: [Sort; 5] = [Sort::Bool, Sort::Pos, Sort::Nat, Sort::Int, Sort::Real];

/// Whether `name` names one of the [`BASIC_SORTS`].
pub(crate) fn is_basic_sort_name(name: &str) -> bool {
    BASIC_SORTS.iter().any(|sort| sort.name() == name)
}

/// Whether `name` uses the reserved `@`-prefix convention every system-generated
/// constructor/mapping/sort declaration.
pub(crate) fn is_system_generated_name(name: &str) -> bool {
    name.starts_with('@')
}
