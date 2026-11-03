#![forbid(unsafe_code)]

use merc_utilities::SourceMap;
use merc_utilities::Span;

use crate::ATerm;
use crate::ATermRef;
use crate::Protected;
use crate::Term;
use crate::storage::ThreadTermPool;

/// An error produced by [ATerm::from_string], pointing at the offending input.
#[derive(Debug, thiserror::Error)]
#[error("{message}\n{snippet}")]
pub struct ParseTermError {
    message: String,
    span: Span,
    /// The caret-annotated source line, rendered eagerly since the error does not borrow the input.
    snippet: String,
}

impl ParseTermError {
    fn new(text: &str, span: Span, message: impl Into<String>) -> Self {
        let mut sources = SourceMap::new();
        sources.add_text("<term>", text);
        ParseTermError {
            message: message.into(),
            snippet: span.render(&sources),
            span,
        }
    }

    /// The byte range of the input at which parsing failed.
    pub fn span(&self) -> &Span {
        &self.span
    }
}

/// An application `name(...)` whose arguments are still being parsed.
struct Frame<'i> {
    name: &'i str,
    /// Index in the argument stack of the first argument of this application.
    base: usize,
}

/// Parses a term of the form `f(t_1, ..., t_n)`, `c` or a natural number, where `%` starts a
/// comment until the end of the line.
///
/// Uses an explicit stack instead of recursion so that arbitrarily deep terms cannot overflow the
/// system stack. All pending arguments share one [Protected] container to avoid protecting every
/// subterm individually.
pub(crate) fn parse_term(tp: &ThreadTermPool, text: &str) -> Result<ATerm, ParseTermError> {
    let mut lexer = Lexer { text, pos: 0 };
    let mut frames: Vec<Frame> = Vec::new();
    let mut args: Protected<Vec<ATermRef<'static>>> = Protected::new(Vec::new());

    'term: loop {
        // Parse the head of a term.
        lexer.skip_trivia();
        let start = lexer.pos;
        let name = lexer.identifier();
        if name.is_empty() {
            return Err(lexer.error_here("expected a term"));
        }

        lexer.skip_trivia();
        let term = if name.bytes().all(|c| c.is_ascii_digit()) {
            if lexer.peek() == Some('(') {
                return Err(ParseTermError::new(
                    text,
                    Span::new(start, lexer.pos + 1),
                    "an integer cannot be applied to arguments",
                ));
            }

            let value = name
                .parse::<usize>()
                .map_err(|_| ParseTermError::new(text, Span::new(start, start + name.len()), "integer is too large"))?;
            tp.create_int(value)
        } else if lexer.eat('(') {
            frames.push(Frame {
                name,
                base: args.read().len(),
            });
            continue 'term;
        } else {
            tp.create_constant(&tp.create_symbol(name, 0))
        };
        args.write(|args| args.push(term.copy()));

        // Close every application that the term just parsed completes.
        loop {
            lexer.skip_trivia();
            let Some(frame) = frames.last() else {
                break 'term;
            };

            if lexer.eat(',') {
                continue 'term;
            } else if lexer.eat(')') {
                let base = frame.base;
                let term = {
                    let read = args.read();
                    tp.create_term_iter(&tp.create_symbol(frame.name, read.len() - base), read[base..].iter())
                };
                frames.pop();

                args.write(|args| {
                    args.truncate(base);
                    args.push(term.copy());
                });
            } else {
                return Err(lexer.error_here("expected ',' or ')'"));
            }
        }
    }

    if lexer.pos < text.len() {
        return Err(lexer.error_here("unexpected input after the term"));
    }

    Ok(args.read()[0].protect())
}

struct Lexer<'i> {
    text: &'i str,
    pos: usize,
}

impl<'i> Lexer<'i> {
    /// Returns the next character without consuming it, or `None` if at the end
    /// of the input.
    fn peek(&self) -> Option<char> {
        self.text[self.pos..].chars().next()
    }

    /// Consumes the expected character if it is present at the current
    /// position.
    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    /// Skips white space and `%` comments.
    fn skip_trivia(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_ascii_whitespace() {
                self.pos += 1;
            } else if c == '%' {
                self.pos = self.text[self.pos..]
                    .find('\n')
                    .map_or(self.text.len(), |i| self.pos + i);
            } else {
                break;
            }
        }
    }

    /// Consumes the longest identifier at the current position, which may be empty.
    fn identifier(&mut self) -> &'i str {
        let start = self.pos;
        let len = self.text[start..]
            .bytes()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'\'')
            .count();
        self.pos += len;
        &self.text[start..self.pos]
    }

    /// An error spanning the character at the current position, or the end of the input.
    fn error_here(&self, message: &str) -> ParseTermError {
        let len = self.peek().map_or(0, char::len_utf8);
        let found = match self.peek() {
            Some(c) => format!("'{c}'"),
            None => "end of input".to_string(),
        };
        ParseTermError::new(
            self.text,
            Span::new(self.pos, self.pos + len),
            format!("{message}, found {found}"),
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::ATerm;
    use crate::ATermInt;
    use crate::ParseTermError;
    use crate::Symb;
    use crate::Term;
    use crate::is_int_term;

    #[test]
    fn test_parse_term() {
        let term = ATerm::from_string("f(a, g(b), c)").unwrap();

        assert_eq!(term.get_head_symbol().name(), "f");
        assert_eq!(term.get_head_symbol().arity(), 3);
        assert_eq!(term.arg(1).get_head_symbol().name(), "g");
        assert_eq!(term.to_string(), "f(a, g(b), c)");
    }

    #[test]
    fn test_parse_term_with_comments_and_white_space() {
        let term = ATerm::from_string("  f( % the first argument\n\ta ,b ) % trailing").unwrap();

        assert_eq!(term, ATerm::from_string("f(a,b)").unwrap());
    }

    #[test]
    fn test_parse_integer() {
        let term = ATerm::from_string("f(42, x1)").unwrap();

        assert!(is_int_term(&term.arg(0)));
        assert_eq!(ATermInt::from(term.arg(0).protect()).value(), 42);
        assert!(!is_int_term(&term.arg(1)));
    }

    #[test]
    fn test_parse_deeply_nested_term() {
        let depth = 100_000;
        let text = format!("{}a{}", "s(".repeat(depth), ")".repeat(depth));

        let mut term = ATerm::from_string(&text).unwrap();
        for _ in 0..depth {
            assert_eq!(term.get_head_symbol().name(), "s");
            term = term.arg(0).protect();
        }
        assert_eq!(term.get_head_symbol().name(), "a");
    }

    #[test]
    fn test_parse_errors_point_at_the_input() {
        let span = |text: &str| {
            let error = ATerm::from_string(text).unwrap_err();
            let error = error.downcast_ref::<ParseTermError>().unwrap();
            (error.span().start, error.span().end)
        };

        assert_eq!(span(""), (0, 0));
        assert_eq!(span("f()"), (2, 3));
        assert_eq!(span("f(a b)"), (4, 5));
        assert_eq!(span("f(a"), (3, 3));
        assert_eq!(span("f(a) b"), (5, 6));
        assert_eq!(span("1(a)"), (0, 2));
        assert_eq!(span("99999999999999999999999"), (0, 23));
    }

    #[test]
    fn test_parse_error_renders_the_source_line() {
        let error = ATerm::from_string("f(a,\n  g(b c))").unwrap_err();
        // Render the parse error itself, since MercError appends a backtrace when RUST_BACKTRACE is set.
        let error = error.downcast_ref::<ParseTermError>().unwrap();

        assert_eq!(
            error.to_string(),
            "expected ',' or ')', found 'c'\n --> 2:7\n  |\n2 |   g(b c))\n  |       ^"
        );
    }
}
