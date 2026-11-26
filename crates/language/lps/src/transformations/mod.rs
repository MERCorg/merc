//! Transformations of linear process specifications, most notably the ones that prepare an LPS for
//! state space exploration.

mod complete_data_specification;
mod instantiate_global_variables;
mod map_summand;
mod normalise_sorts;
mod one_point_rule_rewrite;
mod order_summand_variables;
mod preprocess;
mod replace_constants_by_variables;
mod representative_generator;
mod resolve_name_clashes;

pub use complete_data_specification::complete_data_specification;
pub use instantiate_global_variables::instantiate_global_variables;
pub use normalise_sorts::normalise_sorts;
pub use one_point_rule_rewrite::OnePointEliminations;
pub use one_point_rule_rewrite::one_point_rule_rewrite;
pub use order_summand_variables::order_summand_variables;
pub use preprocess::AppliedTransformation;
pub use preprocess::DEFAULT_PREPROCESSING;
pub use preprocess::LpsTransformation;
pub use preprocess::TransformedLps;
pub use preprocess::preprocess;
pub use replace_constants_by_variables::replace_constants_by_variables;
pub use resolve_name_clashes::resolve_name_clashes;
