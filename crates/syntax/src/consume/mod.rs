use merc_pest_consume::Error;
use merc_pest_consume::match_nodes;
use merc_utilities::Span;

use crate::DataExpr;
use crate::IdDecl;
use crate::Mcrl2Parser;
use crate::Rule;
use crate::Spanned;

mod actfrm;
mod dataexpr;
mod pbesexpr;
mod presexpr;
mod procexpr;
mod regfrm;
mod sortexpr;
mod specs;
mod statefrm;

/// The error type produced while consuming the parse tree.
pub(crate) type ParseResult<T> = std::result::Result<T, Error<Rule>>;
pub(crate) type ParseNode<'i> = merc_pest_consume::Node<'i, Rule, ()>;

merc_pest_consume::declare_parser!(parser = Mcrl2Parser, rule = Rule);

/// Consumes the pest parse tree into syntax tree nodes, split by grammar area into
/// `sortexpr`/`dataexpr`/`procexpr`/`actfrm`/`regfrm`/`statefrm`/`pbesexpr`/`presexpr`/`specs`,
/// each its own `#[merc_pest_consume::parser_methods]` impl block for `Mcrl2Parser`. This module
/// holds only the identifier/variable-list rule methods shared across most of those files.
///
/// Private methods are only called from `match_nodes!` arms within their own file. `pub(crate)`
/// methods are called across files in this module, and/or from the Pratt parsers in
/// `precedence/`.
#[merc_pest_consume::parser_methods]
impl Mcrl2Parser {
    pub(crate) fn Id(identifier: ParseNode) -> ParseResult<Spanned<String>> {
        Ok(Spanned {
            node: identifier.as_str().to_string(),
            span: identifier.as_span().into(),
        })
    }

    pub(crate) fn IdAt(identifier: ParseNode) -> ParseResult<Spanned<String>> {
        Ok(Spanned {
            node: identifier.as_str().to_string(),
            span: identifier.as_span().into(),
        })
    }

    pub(crate) fn IdList(identifiers: ParseNode) -> ParseResult<Vec<(String, Span)>> {
        Ok(identifiers
            .into_children()
            .map(|node| (node.as_str().to_string(), node.as_span().into()))
            .collect())
    }

    pub(crate) fn VarsDeclList(vars: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(vars.into_children();
            [VarsDecl(decl)..] => {
                Ok(decl.flatten().collect())
            },
        )
    }

    fn VarsDecl(decl: ParseNode) -> ParseResult<Vec<IdDecl>> {
        let mut vars = Vec::new();

        match_nodes!(decl.into_children();
            [IdList(identifiers), SortExpr(sort)] => {
                for (id, span) in identifiers {
                    vars.push(IdDecl::new(id, sort.clone(), span));
                }
            },
        );

        Ok(vars)
    }

    pub(crate) fn DataExprList(expr: ParseNode) -> ParseResult<Vec<DataExpr>> {
        match_nodes!(expr.into_children();
            [DataExpr(expr)] => {
                Ok(vec![expr])
            },
            [DataExpr(expr)..] => {
                Ok(expr.collect())
            },
        )
    }
}
