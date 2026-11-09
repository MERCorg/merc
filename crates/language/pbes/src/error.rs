use merc_syntax::SourceMap;
use merc_syntax::Span;

use merc_typecheck::InferenceError;
use merc_typecheck::WellTypedError;

/// An error type checking a whole equation system, such as a PBES or a PRES: a
/// global variable, equation, or `init` declaration that doesn't type check, on
/// top of everything [`WellTypedError`]/[`InferenceError`] already cover for
/// the data-specification subtree.
///
/// More variants may be added, so a caller matching on this needs a catch-all
/// arm.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum EquationSystemError {
    /// An error resolving or checking the data specification itself, or a `glob`/equation-parameter
    /// sort against it.
    #[error(transparent)]
    WellTyped(#[from] WellTypedError),
    /// A sort-inference error checking an embedded data expression, a `PropVarInst` argument, or
    /// a binder.
    #[error(transparent)]
    Inference(#[from] InferenceError),

    #[error("an anonymous structured sort is not allowed in a variable declaration")]
    AnonymousStructInDeclaration { span: Span },

    #[error("the parameter '{name}' of equation '{equation}' is declared more than once")]
    DuplicateEquationParameter { equation: String, name: String, span: Span },
    #[error("the global variable '{name}' is declared more than once")]
    DuplicateGlobalVariable { name: String, span: Span },
    #[error("the propositional variable '{name}' is declared more than once")]
    DuplicatePropositionalVariable { name: String, span: Span },

    #[error("no propositional variable named '{name}' is declared")]
    UndeclaredPropositionalVariable {
        name: String,
        span: Span,
        /// Every declared propositional-variable name.
        candidates: Vec<String>,
    },
    #[error("'{name}' expects {expected} argument(s), found {found}")]
    ArityMismatch {
        name: String,
        expected: usize,
        found: usize,
        span: Span,
    },
}

impl EquationSystemError {
    /// The span of the offending constructed error.
    pub fn span(&self) -> Option<&Span> {
        match self {
            EquationSystemError::WellTyped(error) => error.span(),
            EquationSystemError::Inference(error) => Some(error.span()),
            EquationSystemError::AnonymousStructInDeclaration { span }
            | EquationSystemError::DuplicateEquationParameter { span, .. }
            | EquationSystemError::DuplicateGlobalVariable { span, .. }
            | EquationSystemError::DuplicatePropositionalVariable { span, .. }
            | EquationSystemError::UndeclaredPropositionalVariable { span, .. }
            | EquationSystemError::ArityMismatch { span, .. } => Some(span),
        }
    }

    /// Renders this error's message, followed by a caret-annotated source snippet.
    pub fn render(&self, sources: &SourceMap) -> String {
        match self.span() {
            Some(span) => format!("{self}\n{}", span.render(sources)),
            None => self.to_string(),
        }
    }
}
