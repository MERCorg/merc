#![forbid(unsafe_code)]

use std::hash::Hash;
use std::mem::ManuallyDrop;

use delegate::delegate;

use merc_aterm::ATerm;
use merc_aterm::ATermArgs;
use merc_aterm::ATermIndex;
use merc_aterm::ATermList;
use merc_aterm::ATermRef;
use merc_aterm::Markable;
use merc_aterm::Symbol;
use merc_aterm::SymbolRef;
use merc_aterm::Term;
use merc_aterm::TermIterator;
use merc_aterm::Transmutable;
use merc_aterm::storage::Marker;
use merc_data::BasicSort;
use merc_data::DataExpression;
use merc_data::DataVariable;
use merc_data::SortExpression;
use merc_macros::merc_derive_terms;
use merc_macros::merc_term;

thread_local! {
    pub(crate) static LPS_SYMBOLS: LpsSymbols = LpsSymbols::new();
}

/// The function symbols of the mCRL2 terms that are specific to an LPS in its binary format, see
/// `libraries/core/include/mcrl2/core/detail/function_symbols.h` of the mCRL2 toolset.
///
/// The symbols are wrapped in `ManuallyDrop` so that their destructors never run at thread exit.
pub(crate) struct LpsSymbols {
    pub marker: ManuallyDrop<Symbol>,
    distribution: ManuallyDrop<Symbol>,
    initializer: ManuallyDrop<Symbol>,
}

impl LpsSymbols {
    fn new() -> Self {
        LpsSymbols {
            marker: ManuallyDrop::new(Symbol::new("linear_process_specification", 0)),
            distribution: ManuallyDrop::new(Symbol::new("Distribution", 2)),
            initializer: ManuallyDrop::new(Symbol::new("LinearProcessInit", 2)),
        }
    }
}

#[merc_derive_terms]
mod inner {
    use merc_macros::merc_ignore;

    use super::*;

    /// The distribution of a summand or initial process, as `Distribution(variables, expression)`.
    #[merc_term(is_distribution)]
    pub struct Distribution {
        term: ATerm,
    }

    impl Distribution {
        /// The distribution of a non-stochastic summand or initial process:
        /// `Distribution([], @NoValue)`.
        #[merc_ignore]
        pub fn none() -> Self {
            let no_value = DataVariable::with_sort("@NoValue", SortExpression::from(BasicSort::new("@NoValue")).copy());
            LPS_SYMBOLS.with(|symbols| Distribution {
                term: ATerm::with_args(
                    &*symbols.distribution,
                    &[ATermList::<ATerm>::empty().into(), ATerm::from(no_value)],
                )
                .protect(),
            })
        }

        /// The variables of the distribution, which are empty unless it is stochastic.
        pub fn variables(&self) -> ATermList<DataVariable> {
            self.term.arg(0).into()
        }
    }

    /// The initial process of an LPS, as `LinearProcessInit(values, distribution)`.
    #[merc_term(is_linear_process_init)]
    pub struct LinearProcessInit {
        term: ATerm,
    }

    impl LinearProcessInit {
        /// Creates the initial process with the given values of the process parameters.
        #[merc_ignore]
        pub fn new(initial_state: ATermList<DataExpression>, distribution: Distribution) -> Self {
            LPS_SYMBOLS.with(|symbols| LinearProcessInit {
                term: ATerm::with_args(
                    &*symbols.initializer,
                    &[ATerm::from(initial_state), ATerm::from(distribution)],
                )
                .protect(),
            })
        }

        /// The initial value of every process parameter.
        pub fn initial_state(&self) -> ATermList<DataExpression> {
            self.term.arg(0).into()
        }

        /// The distribution of the initial process.
        pub fn distribution(&self) -> DistributionRef<'_> {
            self.term.arg(1).into()
        }
    }
}

pub(crate) use inner::*;

/// Returns true iff the head symbol of `term` is that of a [Distribution].
fn is_distribution<'a, 'b, T: Term<'a, 'b>>(term: &'b T) -> bool {
    LPS_SYMBOLS.with(|symbols| term.get_head_symbol() == symbols.distribution.copy())
}

/// Returns true iff the head symbol of `term` is that of a [LinearProcessInit].
fn is_linear_process_init<'a, 'b, T: Term<'a, 'b>>(term: &'b T) -> bool {
    LPS_SYMBOLS.with(|symbols| term.get_head_symbol() == symbols.initializer.copy())
}
