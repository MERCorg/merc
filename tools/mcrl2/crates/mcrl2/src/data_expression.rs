use mcrl2_sys::data::ffi::mcrl2_data_expression_is_abstraction;
use mcrl2_sys::data::ffi::mcrl2_data_expression_is_application;
use mcrl2_sys::data::ffi::mcrl2_data_expression_is_data_expression;
use mcrl2_sys::data::ffi::mcrl2_data_expression_is_function_symbol;
use mcrl2_sys::data::ffi::mcrl2_data_expression_is_machine_number;
use mcrl2_sys::data::ffi::mcrl2_data_expression_is_untyped_identifier;
use mcrl2_sys::data::ffi::mcrl2_data_expression_is_variable;
use mcrl2_sys::data::ffi::mcrl2_data_expression_is_where_clause;

use mcrl2_macros::mcrl2_derive_terms;
use mcrl2_sys::data::ffi::mcrl2_is_data_sort_expression;

use crate::ATermRef;

/// Checks if this term is a data variable.
pub fn is_variable(term: &ATermRef<'_>) -> bool {
    term.require_valid();
    mcrl2_data_expression_is_variable(term.get())
}

/// Checks if this term is a data application.
pub fn is_application(term: &ATermRef<'_>) -> bool {
    term.require_valid();
    mcrl2_data_expression_is_application(term.get())
}

/// Checks if this term is a data abstraction.
pub fn is_abstraction(term: &ATermRef<'_>) -> bool {
    term.require_valid();
    mcrl2_data_expression_is_abstraction(term.get())
}

/// Checks if this term is a data function symbol.
pub fn is_function_symbol(term: &ATermRef<'_>) -> bool {
    term.require_valid();
    mcrl2_data_expression_is_function_symbol(term.get())
}

/// Checks if this term is a data where clause.
pub fn is_where_clause(term: &ATermRef<'_>) -> bool {
    term.require_valid();
    mcrl2_data_expression_is_where_clause(term.get())
}

/// Checks if this term is a data machine number.
pub fn is_machine_number(term: &ATermRef<'_>) -> bool {
    term.require_valid();
    mcrl2_data_expression_is_machine_number(term.get())
}

/// Checks if this term is a data untyped identifier.
pub fn is_untyped_identifier(term: &ATermRef<'_>) -> bool {
    term.require_valid();
    mcrl2_data_expression_is_untyped_identifier(term.get())
}

/// Checks if this term is a data expression.
pub fn is_data_expression(term: &ATermRef<'_>) -> bool {
    term.require_valid();
    mcrl2_data_expression_is_data_expression(term.get())
}

/// Checks if this term is a sort expression.
pub fn is_sort_expression(term: &ATermRef<'_>) -> bool {
    term.require_valid();
    mcrl2_is_data_sort_expression(term.get())
}

// This module is only used internally to run the proc macro.
#[mcrl2_derive_terms]
mod inner {
    use std::fmt;

    use mcrl2_macros::{mcrl2_ignore, mcrl2_term};
    use mcrl2_sys::data::ffi::mcrl2_data_expression_to_string;

    use crate::ATermArgs;
    use crate::ATermRef;
    use crate::Markable;
    use crate::Todo;
    use crate::is_machine_number;
    use crate::is_sort_expression;
    use crate::{
        ATerm, ATermString, is_abstraction, is_application, is_data_expression, is_function_symbol,
        is_variable,
    };

    /// Represents a data::data_expression from the mCRL2 toolset.
    ///  A data expression can be any of:
    ///     - a variable
    ///     - a function symbol, i.e. f without arguments.
    ///     - a term applied to a number of arguments, i.e., t_0(t1, ..., tn).
    ///     - an abstraction lambda x: Sort . e, or forall and exists.
    ///     - machine number, a value [0, ..., 2^64-1].
    ///
    /// Not supported:
    ///     - a where clause "e where [x := f, ...]"
    ///     - set enumeration
    ///     - bag enumeration
    ///
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

        /// Returns the head symbol a data expression
        ///     - function symbol                  f -> f
        ///     - application       f(t_0, ..., t_n) -> f
        pub fn data_function_symbol(&self) -> DataFunctionSymbolRef<'_> {
            if is_application(&self.term) {
                self.term.arg(0).upgrade(&self.term).into()
            } else if is_function_symbol(&self.term) {
                self.term.copy().into()
            } else {
                panic!("data_function_symbol not implemented for {}", self);
            }
        }

        /// Returns the arguments of a data expression
        ///     - function symbol                  f -> []
        ///     - application       f(t_0, ..., t_n) -> [t_0, ..., t_n]
        pub fn data_arguments(&self) -> ATermArgs<'_> {
            if is_application(&self.term) {
                let mut result = self.term.arguments();
                result.next();
                result
            } else if is_function_symbol(&self.term) {
                Default::default()
            } else {
                panic!("data_arguments not implemented for {}", self);
            }
        }

        /// Returns the arguments of a data expression
        ///     - function symbol                  f -> []
        ///     - application       f(t_0, ..., t_n) -> [t_0, ..., t_n]
        pub fn data_sort(&self) -> SortExpression {
            if is_function_symbol(&self.term) {
                DataFunctionSymbolRef::from(self.term.copy()).sort().protect()
            } else if is_variable(&self.term) {
                DataVariableRef::from(self.term.copy()).sort()
            } else {
                panic!("data_sort not implemented for {}", self);
            }
        }

        /// Pretty prints the data expression.
        pub fn pretty_print(&self) -> String {
            mcrl2_data_expression_to_string(&self.term.get())
        }
    }

    impl fmt::Display for DataExpression {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if is_function_symbol(&self.term) {
                write!(f, "{}", DataFunctionSymbolRef::from(self.term.copy()))
            } else if is_application(&self.term) {
                write!(f, "{}", DataApplicationRef::from(self.term.copy()))
            } else if is_variable(&self.term) {
                write!(f, "{}", DataVariableRef::from(self.term.copy()))
            } else if is_machine_number(&self.term) {
                write!(f, "{}", MachineNumberRef::from(self.term.copy()))
            } else {
                write!(f, "{}", self.term)
            }
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
        pub fn sort(&self) -> SortExpression {
            SortExpression::new(self.term.arg(1).protect())
        }
    }

    impl fmt::Display for DataVariable {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}: {}", self.name(), self.sort())
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

        /// Returns the head symbol a data application
        pub fn data_function_symbol(&self) -> DataFunctionSymbolRef<'_> {
            self.term.arg(0).upgrade(&self.term).into()
        }

        /// Returns the arguments of a data application
        pub fn data_arguments(&self) -> ATermArgs<'_> {
            let mut result = self.term.arguments();
            result.next();
            result
        }

        /// Returns the sort of a data application.
        pub fn sort(&self) -> SortExpressionRef<'_> {
            // We only change the lifetime, but that is fine since it is derived from the current term.
            unsafe { std::mem::transmute(SortExpressionRef::from(self.term.arg(0))) }
        }        
    }

    impl fmt::Display for DataApplication {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.data_function_symbol())?;

            // Write no brackets when there are no arguments.
            let mut first = true;
            for arg in self.data_arguments() {
                if !first {
                    write!(f, ", ")?;
                } else {
                    write!(f, "(")?;
                }

                write!(f, "{}", DataExpressionRef::from(arg.copy()))?;
                first = false;
            }

            if !first {
                write!(f, ")")?;
            }

            Ok(())
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

        /// Returns the sort of the function symbol.
        pub fn sort(&self) -> SortExpressionRef<'_> {
            self.term.arg(1).into()
        }

        /// Returns the name of the function symbol
        pub fn name(&self) -> &str {
            // We only change the lifetime, but that is fine since it is derived from the current term.
            unsafe { std::mem::transmute(self.term.arg(0).get_head_symbol().name()) }
        }
    }

    impl fmt::Display for DataFunctionSymbol {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if !self.is_default() {
                write!(f, "{}", self.name())
            } else {
                write!(f, "<default>")
            }
        }
    }

    /// Represents a data::sort_expression from the mCRL2 toolset.   
    #[mcrl2_term(is_sort_expression)]
    pub struct SortExpression {
        term: ATerm,
    }

    impl SortExpression {
        /// Creates a new data::sort_expression from the given term.
        #[mcrl2_ignore]
        pub fn new(term: ATerm) -> Self {
            debug_assert!(is_sort_expression(&term.copy()));
            SortExpression { term }
        }

        /// Returns the name of the sort.
        pub fn name(&self) -> &str {
            // We only change the lifetime, but that is fine since it is derived from the current term.
            unsafe { std::mem::transmute(self.term.arg(0).get_head_symbol().name()) }
        }
    }

    impl fmt::Display for SortExpression {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.name())
        }
    }
    
    /// Represents a data::machine_number from the mCRL2 toolset.
    #[mcrl2_term(is_machine_number)]
    struct MachineNumber {
        pub term: ATerm,
    }

    impl MachineNumber {
        /// Obtain the underlying value of a machine number.
        pub fn value(&self) -> u64 {
            0
        }
    }

    impl fmt::Display for MachineNumber {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.value())
        }
    }

}

pub use inner::*;

impl From<DataVariable> for DataExpression {
    fn from(var: DataVariable) -> Self {
        DataExpression::new(var.into())
    }
}