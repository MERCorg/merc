use std::fmt;

use mcrl2_sys::data::ffi::mcrl2_is_variable;
use mcrl2_sys::data::ffi::mcrl2_variable_name;
use mcrl2_sys::data::ffi::mcrl2_variable_sort;
use mcrl2_sys::data::ffi::variable;

use crate::ATerm;
use crate::ATermString;

/// Represents a data::variable from the mCRL2 toolset.
pub struct DataVariable {
    term: UniquePtr<variable>,
}

impl DataVariable {

    /// Returns the name of the variable.
    pub fn name(&self) -> ATermString {
        ATermString::new(mcrl2_variable_name(&self.term).unwrap())
    }

    /// Returns the sort of the variable.
    pub fn sort(&self) -> DataSort {
        DataSort::new(mcrl2_variable_sort(&self.term).unwrap())
    }

    /// Creates a new data::variable from the given aterm.
    pub(crate) fn new(term: ATerm) -> Self {
        debug_assert!(
            mcrl2_is_variable(&term.get()),
            "The term {:?} is not a variable.",
            term
        );

        DataVariable { term }
    }
}

impl fmt::Debug for DataVariable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.name())
    }
}

impl Into<ATerm> for DataVariable {
    fn into(self) -> ATerm {
        self.term
    }
}

/// Represents a data::sort from the mCRL2 toolset.
pub struct DataSort {
    term: ATerm,
}

impl DataSort {
    /// Creates a new data::sort from the given term.
    pub(crate) fn new(term: UniquePtr<sort_expression>) -> Self {
        DataSort {
            term: ATerm::new(term.into()),
        }
    }
}

/// Represents a data::data_expression from the mCRL2 toolset.
pub struct DataExpression {
    term: ATerm,
}

impl DataExpression {
    /// Creates a new data::data_expression from the given term.
    pub(crate) fn new(term: UniquePtr<data_expression>) -> Self {
        DataExpression {
            term: ATerm::new(term.into()),
        }
    }
}