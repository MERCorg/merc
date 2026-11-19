#![doc = include_str!("../README.md")]

mod lowering;
mod system_symbols;

#[cfg(test)]
mod instantiation_tests;

pub use lowering::lower_data_specification;
pub use lowering::lower_expression;
