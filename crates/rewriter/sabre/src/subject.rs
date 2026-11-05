#![forbid(unsafe_code)]

use merc_data::DataApplication;
use merc_data::DataExpression;
use merc_data::DataExpressionRef;
use merc_data::DataFunctionSymbolRef;
use merc_data::is_data_machine_number;

use crate::set_automaton::machine_number_symbol;
use crate::utilities::DataPosition;
use crate::utilities::DataPositionIndexed;

/// The term that a rule is matched against, and whose variables instantiate its right-hand side
/// and conditions.
///
/// The stack machine gives a redex as its head symbol applied to the arguments in the top slots
/// of its term stack, so that a redex that a rule rewrites is never constructed.
pub enum Subject<'a> {
    Term(&'a DataExpressionRef<'a>),
    /// A symbol applied to arguments, each of which is present.
    Arguments(DataFunctionSymbolRef<'a>, &'a [Option<DataExpressionRef<'a>>]),
}

impl<'a> Subject<'a> {
    pub fn arity(&self) -> usize {
        match self {
            Subject::Term(term) => term.data_arguments().len(),
            Subject::Arguments(_, arguments) => arguments.len(),
        }
    }

    pub fn argument(&self, index: usize) -> DataExpressionRef<'a> {
        match self {
            Subject::Term(term) => term.data_arg(index),
            Subject::Arguments(_, arguments) => arguments[index]
                .as_ref()
                .expect("every argument of a subject is present")
                .copy(),
        }
    }

    pub fn arguments(&self) -> impl Iterator<Item = DataExpressionRef<'a>> + '_ {
        (0..self.arity()).map(|index| self.argument(index))
    }

    /// Returns the operation id of the function symbol at `position` as the set automaton sees
    /// it, or `None` for a variable. A machine number has no function symbol, so it is seen as
    /// the stand-in symbol that the automaton reserves for them.
    pub fn operation_at(&self, position: &DataPosition) -> Option<usize> {
        if let Subject::Arguments(symbol, _) = self
            && position.is_empty()
        {
            return Some(symbol.operation_id());
        }

        let term = self.get_data_position(position);
        if is_data_machine_number(&term) {
            Some(machine_number_symbol().operation_id())
        } else {
            Some(term.try_data_function_symbol()?.operation_id())
        }
    }

    /// Returns the subject as a term, constructing it if needed.
    pub fn to_term(&self) -> DataExpression {
        match self {
            Subject::Term(term) => term.protect(),
            Subject::Arguments(symbol, []) => symbol.protect().into(),
            Subject::Arguments(symbol, arguments) => {
                DataApplication::with_iter(symbol, arguments.len(), arguments.iter().flatten()).into()
            }
        }
    }
}

impl<'b> DataPositionIndexed<'b> for Subject<'b> {
    type Target<'a>
        = DataExpressionRef<'a>
    where
        Self: 'a;

    /// Returns the subterm at `position`, which must not be the root of a subject given by its
    /// arguments.
    fn get_data_position(&'b self, position: &DataPosition) -> Self::Target<'b> {
        match self {
            Subject::Term(term) => term.get_data_position(position),
            Subject::Arguments(..) => {
                let [first, rest @ ..] = position.indices() else {
                    panic!("the root of a subject given by its arguments is not a term");
                };

                let mut result = self.argument(*first - 1);
                for index in rest {
                    result = result.data_arg(*index - 1); // Positions are 1 indexed.
                }
                result
            }
        }
    }
}
