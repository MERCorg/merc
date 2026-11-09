use std::collections::HashSet;

use merc_aterm::ATerm;
use merc_aterm::Symb;
use merc_aterm::Term;
use merc_data::is_data_function_symbol;
use merc_typecheck::NumberEncoding;

use crate::system_symbols::C_DUB;
use crate::system_symbols::C_NAT;
use crate::system_symbols::C1;

/// Determines the number encoding of already lowered `terms`, from the constructors that their
/// numbers use.
///
/// Only the binary encoding has these constructors, so terms without any number are reported as
/// [`NumberEncoding::MachineWord`], which is the encoding the mCRL2 toolset uses.
pub fn number_encoding<I>(terms: I) -> NumberEncoding
where
    I: IntoIterator<Item = ATerm>,
{
    let mut visited: HashSet<ATerm> = HashSet::new();
    let mut stack: Vec<ATerm> = terms.into_iter().collect();

    while let Some(term) = stack.pop() {
        if !visited.insert(term.clone()) {
            continue;
        }

        if is_data_function_symbol(&term) {
            if matches!(term.arg(0).get_head_symbol().name(), C1 | C_DUB | C_NAT) {
                return NumberEncoding::Binary;
            }
        } else {
            stack.extend(term.arguments().map(|argument| argument.protect()));
        }
    }

    NumberEncoding::MachineWord
}

#[cfg(test)]
mod tests {
    use merc_aterm::ATerm;
    use merc_syntax::DataExpr;
    use merc_syntax::SourceMap;
    use merc_syntax::UntypedDataSpecification;
    use merc_typecheck::DataSpecification;
    use merc_typecheck::NumberEncoding;

    use crate::lower_expression;
    use crate::number_encoding;

    /// The lowered literal `5`, in the given encoding.
    fn number(encoding: NumberEncoding) -> ATerm {
        let untyped = UntypedDataSpecification::parse("map n: Nat;").unwrap();
        let mut spec = DataSpecification::from_untyped_with(untyped, encoding, &mut SourceMap::new()).unwrap();
        let typed = spec.typecheck_expression(&DataExpr::parse("5").unwrap()).unwrap();
        lower_expression(&spec, &typed).into()
    }

    #[test]
    #[cfg_attr(miri, ignore)] // pest calls into psm, which miri does not support.
    fn test_number_encoding() {
        assert_eq!(
            number_encoding([number(NumberEncoding::Binary)]),
            NumberEncoding::Binary
        );
        assert_eq!(
            number_encoding([number(NumberEncoding::MachineWord)]),
            NumberEncoding::MachineWord
        );
        assert_eq!(number_encoding([]), NumberEncoding::MachineWord);
    }
}
