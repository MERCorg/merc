use merc_pest_consume::match_nodes;
use merc_utilities::Span;

use crate::Condition;
use crate::DataExpr;
use crate::Eq;
use crate::IdDecl;
use crate::Mcrl2Parser;
use crate::PresExpr;
use crate::PresExprKind;
use crate::Rule;
use crate::parse_presexpr;

use super::ParseNode;
use super::ParseResult;

#[merc_pest_consume::parser_methods]
impl Mcrl2Parser {
    pub(crate) fn PresExpr(expr: ParseNode) -> ParseResult<PresExpr> {
        parse_presexpr(expr.children().as_pairs().clone())
    }

    pub(crate) fn PresExprEqinf(input: ParseNode) -> ParseResult<PresExpr> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [PresExpr(body)] => {
                Ok(PresExprKind::Equal {
                    eq: Eq::EqInf,
                    body: Box::new(body),
                }.spanned(span))
            },
        )
    }

    pub(crate) fn PresExprEqninf(input: ParseNode) -> ParseResult<PresExpr> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [PresExpr(body)] => {
                Ok(PresExprKind::Equal {
                    eq: Eq::EqnInf,
                    body: Box::new(body),
                }.spanned(span))
            },
        )
    }

    pub(crate) fn PresExprCondsm(input: ParseNode) -> ParseResult<PresExpr> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [PresExpr(expr), PresExpr(then), PresExpr(else_)] => {
                Ok(PresExprKind::Condition{
                    condition: Condition::Condsm,
                    lhs: Box::new(expr),
                    then: Box::new(then),
                    else_: Box::new(else_),
                }.spanned(span))
            },
        )
    }

    pub(crate) fn PresExprCondeq(input: ParseNode) -> ParseResult<PresExpr> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [PresExpr(expr), PresExpr(then), PresExpr(else_)] => {
                Ok(PresExprKind::Condition{
                    condition: Condition::Condeq,
                    lhs: Box::new(expr),
                    then: Box::new(then),
                    else_: Box::new(else_),
                }.spanned(span))
            },
        )
    }

    pub(crate) fn PresExprInf(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(variables)] => {
                Ok(variables)
            },
        )
    }

    pub(crate) fn PresExprSup(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(variables)] => {
                Ok(variables)
            },
        )
    }

    pub(crate) fn PresExprSum(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(variables)] => {
                Ok(variables)
            },
        )
    }

    pub(crate) fn PresExprLeftConstantMultiply(input: ParseNode) -> ParseResult<DataExpr> {
        match_nodes!(input.into_children();
            [DataValExpr(constant)] => {
                Ok(constant)
            },
        )
    }

    pub(crate) fn PresExprRightConstMultiply(input: ParseNode) -> ParseResult<DataExpr> {
        match_nodes!(input.into_children();
            [DataValExpr(constant)] => {
                Ok(constant)
            },
        )
    }
}
