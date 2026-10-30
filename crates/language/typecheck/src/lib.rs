mod builtins;
mod checking;
mod data_specification;
mod inference;
mod ir;
mod lowering;
mod modal;
mod number_encoding;
mod process;
mod resolution;
mod signature;
mod typing_info;

// The internal passes are flattened to the crate root for convenience; their
// exact module is not part of the interface. Only the items below marked `pub`
// are exposed outside the crate.
pub(crate) use builtins::*;
pub(crate) use data_specification::*;
pub(crate) use inference::*;
#[allow(unused_imports)]
pub(crate) use ir::*;
#[allow(unused_imports)]
pub(crate) use lowering::*;
pub(crate) use resolution::*;
#[allow(unused_imports)]
pub(crate) use signature::*;

pub use data_specification::DataSpecification;
pub use data_specification::TypedDataSpecification;
pub use inference::InferenceError;
pub use modal::Equation;
pub use modal::FormulaType;
pub use modal::ModalEquationSystem;
pub use modal::ModalError;
pub use modal::ModalSpecification;
pub use number_encoding::NumberEncoding;
pub use process::ProcessError;
pub use process::ProcessSpecification;
pub use process::disambiguate_process_specification;
pub use signature::WellTypedError;
pub use typing_info::ResolvedName;
pub use typing_info::TypedNode;
pub use typing_info::TypingInfo;
pub(crate) use typing_info::declared_span;

/// The building blocks for type checking a specification defined in another crate, such as the
/// PBES and PRES specifications of `merc_pbes` and `merc_pres`: variable resolution, scoped expression checking, and
/// recording typing information. These are implementation details of the type checker and not a
/// stable interface, hence only available with the `internal` feature.
#[cfg(feature = "internal")]
#[doc(hidden)]
pub mod internal {
    use merc_syntax::Span;

    use crate::DataSpecification;
    use crate::ResolvedName;
    use crate::TypingInfo;

    pub use crate::inference::ResolvedSortId;
    pub use crate::inference::ResolvedSortTag;
    pub use crate::resolution::NameStack;
    pub use crate::resolution::Scope as VariableScope;
    pub use crate::resolution::resolve_in_data_expr;
    pub use crate::resolution::resolve_in_prop_var_inst;
    pub use crate::typing_info::SortReference;
    pub use crate::typing_info::collect_sort_name_references;
    pub use crate::typing_info::declared_span;
    pub use crate::typing_info::push_sort_references;

    /// The resolved `Bool` sort of `data`.
    pub fn bool_sort(data: &DataSpecification) -> ResolvedSortId {
        data.context().sorts.bool_sort()
    }

    /// The resolved `Real` sort of `data`.
    pub fn real_sort(data: &DataSpecification) -> ResolvedSortId {
        data.context().sorts.real_sort()
    }

    /// Records that the occurrence at `span` resolves to `name`.
    pub fn push_resolved_name(typing: &mut TypingInfo, span: Span, name: ResolvedName) {
        typing.push(span, name);
    }

    /// Shared building blocks for checking expressions in the scope of a declaration.
    pub mod checking {
        pub use crate::checking::Scope;
        pub use crate::checking::check_expression_against;
        pub use crate::checking::collect_binder_sorts;
        pub use crate::checking::declare_variables;
        pub use crate::checking::resolve_declared_sort;
    }
}
