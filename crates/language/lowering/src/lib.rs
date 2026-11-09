//! Lowers a type checked mCRL2 data specification, see [merc_typecheck::DataSpecification], to the
//! aterm representation of [merc_data] used by the rewriters and the rest of merc.

mod lowering;
mod number_encoding;
mod system_symbols;

#[cfg(test)]
mod instantiation_tests;

pub use lowering::lower_data_specification;
pub use lowering::lower_expression;
pub use number_encoding::number_encoding;
