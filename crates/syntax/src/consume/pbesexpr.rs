use merc_pest_consume::match_nodes;

use crate::IdDecl;
use crate::Mcrl2Parser;
use crate::PbesExpr;
use crate::Rule;
use crate::parse_pbesexpr;

use super::ParseNode;
use super::ParseResult;

#[merc_pest_consume::parser_methods]
impl Mcrl2Parser {
    pub(crate) fn PbesExpr(expr: ParseNode) -> ParseResult<PbesExpr> {
        parse_pbesexpr(expr.children().as_pairs().clone())
    }

    pub(crate) fn PbesExprForall(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(vars)] => {
                Ok(vars)
            },
        )
    }

    pub(crate) fn PbesExprExists(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(vars)] => {
                Ok(vars)
            },
        )
    }
}
