use std::fmt;

/// A struct that can be used to pretty print the parse tree obtained from the [pest] crate.
pub struct DisplayPair<'i, R: pest::RuleType>(pub pest::iterators::Pair<'i, R>);

impl<R: pest::RuleType> fmt::Display for DisplayPair<'_, R> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.display(f, 0)
    }
}

impl<R: pest::RuleType> DisplayPair<'_, R> {
    fn display(&self, f: &mut fmt::Formatter, depth: usize) -> fmt::Result {
        let span = self.0.clone().as_span();
        let rule = self.0.as_rule();
        let inner = self.0.clone().into_inner();
        let indent = "  ".repeat(depth);
        let children_possible = if let Some(len) = inner.size_hint().1 {
            len > 0
        } else {
            true
        };
        // `size_hint` borrows `inner`, so it can still be consumed below.

        write!(
            f,
            "{}{:?}({}, {}, \"{}\"",
            indent,
            rule,
            span.start_pos().pos(),
            span.end_pos().pos(),
            span.as_str()
        )?;
        if children_possible {
            writeln!(f, ", [")?;
            for pair in inner {
                DisplayPair(pair).display(f, depth + 1)?;
            }
            write!(f, "{indent}]),")?;
        } else {
            write!(f, ")")?;
        }

        if depth > 0 {
            writeln!(f)?;
        }

        Ok(())
    }
}
