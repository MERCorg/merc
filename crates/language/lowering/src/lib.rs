//! Lowers a type checked mCRL2 data specification, see [merc_typecheck::DataSpecification], to the
//! aterm representation of [merc_data] used by the rewriters and the rest of merc.

mod mcrl2_lowering;
mod system_symbols;

#[cfg(test)]
mod instantiation_tests;

pub use mcrl2_lowering::lower_data_specification;
pub use mcrl2_lowering::lower_expression;
