#![allow(dead_code)]
use std::fmt;

use mcrl2_sys::data::ffi::mcrl2_data_expression_is_abstraction;
use mcrl2_sys::data::ffi::mcrl2_data_expression_is_application;
use mcrl2_sys::data::ffi::mcrl2_data_expression_is_data_expression;
use mcrl2_sys::data::ffi::mcrl2_data_expression_is_function_symbol;
use mcrl2_sys::data::ffi::mcrl2_data_expression_is_machine_number;
use mcrl2_sys::data::ffi::mcrl2_data_expression_is_untyped_identifier;
use mcrl2_sys::data::ffi::mcrl2_data_expression_is_variable;
use mcrl2_sys::data::ffi::mcrl2_data_expression_is_where_clause;

use mcrl2_macros::mcrl2_derive_terms;

use crate::ATermRef;

/// Checks if this term is a data variable.
pub fn is_variable(term: &ATermRef<'_>) -> bool {
    mcrl2_data_expression_is_variable(term.get())
}

/// Checks if this term is a data application.
pub fn is_application(term: &ATermRef<'_>) -> bool {
    mcrl2_data_expression_is_application(term.get())
}

/// Checks if this term is a data abstraction.
pub fn is_abstraction(term: &ATermRef<'_>) -> bool {
    mcrl2_data_expression_is_abstraction(term.get())
}

/// Checks if this term is a data function symbol.
pub fn is_function_symbol(term: &ATermRef<'_>) -> bool {
    mcrl2_data_expression_is_function_symbol(term.get())
}

/// Checks if this term is a data where clause.
pub fn is_where_clause(term: &ATermRef<'_>) -> bool {
    mcrl2_data_expression_is_where_clause(term.get())
}

/// Checks if this term is a data machine number.
pub fn is_machine_number(term: &ATermRef<'_>) -> bool {
    mcrl2_data_expression_is_machine_number(term.get())
}

/// Checks if this term is a data untyped identifier.
pub fn is_untyped_identifier(term: &ATermRef<'_>) -> bool {
    mcrl2_data_expression_is_untyped_identifier(term.get())
}

/// Checks if this term is a data expression.
pub fn is_data_expression(term: &ATermRef<'_>) -> bool {
    mcrl2_data_expression_is_data_expression(term.get())
}

// This module is only used internally to run the proc macro.
#[mcrl2_derive_terms]
mod inner {
    use mcrl2_macros::{mcrl2_ignore, mcrl2_term};

    use crate::ATermRef;
    use crate::Markable;
    use crate::Todo;
    use crate::{
        ATerm, ATermString, DataSort, is_abstraction, is_application, is_data_expression, is_function_symbol,
        is_variable,
    };

    /// Represents a data::data_expression from the mCRL2 toolset.
    #[mcrl2_term(is_data_expression)]
    pub struct DataExpression {
        term: ATerm,
    }

    impl DataExpression {
        /// Creates a new data::data_expression from the given term.
        #[mcrl2_ignore]
        pub fn new(term: ATerm) -> Self {
            Self { term }
        }
    }

    /// Represents a data::variable from the mCRL2 toolset.
    #[mcrl2_term(is_variable)]
    pub struct DataVariable {
        term: ATerm,
    }

    impl DataVariable {
        /// Creates a new data::variable from the given aterm.
        #[mcrl2_ignore]
        pub fn new(term: ATerm) -> Self {
            debug_assert!(is_variable(&term.copy()));
            DataVariable { term }
        }

        /// Returns the name of the variable.
        pub fn name(&self) -> ATermString {
            ATermString::new(self.term.arg(0).protect())
        }

        /// Returns the sort of the variable.
        pub fn sort(&self) -> DataSort {
            DataSort::new(self.term.arg(1).protect())
        }
    }

    /// Represents a data::application from the mCRL2 toolset.
    #[mcrl2_term(is_application)]
    pub struct DataApplication {
        term: ATerm,
    }

    impl DataApplication {
        /// Creates a new data::application from the given term.
        #[mcrl2_ignore]
        pub(crate) fn new(term: ATerm) -> Self {
            debug_assert!(is_application(&term.copy()));
            DataApplication { term }
        }
    }

    /// Represents a data::abstraction from the mCRL2 toolset.
    #[mcrl2_term(is_application)]
    pub struct DataAbstraction {
        term: ATerm,
    }

    impl DataAbstraction {
        /// Creates a new data::abstraction from the given term.
        #[mcrl2_ignore]
        pub(crate) fn new(term: ATerm) -> Self {
            debug_assert!(is_abstraction(&term.copy()));
            DataAbstraction { term }
        }
    }

    /// Represents a data::function_symbol from the mCRL2 toolset.
    #[mcrl2_term(is_function_symbol)]
    pub struct DataFunctionSymbol {
        term: ATerm,
    }

    impl DataFunctionSymbol {
        /// Creates a new data::function_symbol from the given term.
        #[mcrl2_ignore]
        pub(crate) fn new(term: ATerm) -> Self {
            debug_assert!(is_function_symbol(&term.copy()));
            DataFunctionSymbol { term }
        }
    }
}

pub use inner::*;

impl From<DataVariable> for DataExpression {
    fn from(var: DataVariable) -> Self {
        DataExpression::new(var.into())
    }
}

impl fmt::Display for DataVariable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {:?}", self.name(), self.sort())
    }
}
