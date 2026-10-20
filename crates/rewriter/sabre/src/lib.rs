#![doc = include_str!("../README.md")]

mod innermost_rewriter;
mod naive_rewriter;
mod rewrite_specification;
mod sabre_rewriter;
mod set_automaton;

pub mod matching;
pub mod test_utility;
pub mod utilities;

pub use innermost_rewriter::AnnouncementInnermost;
pub use innermost_rewriter::InnermostRewriter;
pub use naive_rewriter::NaiveRewriter;
pub use rewrite_specification::Condition;
pub use rewrite_specification::RewriteSpecification;
pub use rewrite_specification::Rule;
pub use sabre_rewriter::RewriteEngine;
pub use sabre_rewriter::RewritingStatistics;
pub use sabre_rewriter::SabreRewriter;
pub use set_automaton::SetAutomaton;
pub use set_automaton::is_supported_rule;
