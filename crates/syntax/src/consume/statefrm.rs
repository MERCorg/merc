use merc_pest_consume::match_nodes;
use merc_utilities::Span;

use crate::DataExpr;
use crate::IdDecl;
use crate::Mcrl2Parser;
use crate::RegFrm;
use crate::Rule;
use crate::StateFrm;
use crate::StateFrmKind;
use crate::StateVarAssignment;
use crate::StateVarDecl;
use crate::parse_statefrm;

use super::ParseNode;
use super::ParseResult;

#[merc_pest_consume::parser_methods]
impl Mcrl2Parser {
    pub(crate) fn StateFrmId(id: ParseNode) -> ParseResult<StateFrm> {
        let span: Span = id.as_span().into();
        match_nodes!(id.into_children();
            [Id(identifier)] => {
                Ok(StateFrmKind::Id(identifier.node, Vec::new()).spanned(span))
            },
            [Id(identifier), DataExprList(expressions)] => {
                Ok(StateFrmKind::Id(identifier.node, expressions).spanned(span))
            },
        )
    }

    pub(crate) fn StateFrmExists(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(variables)] => {
                Ok(variables)
            },
        )
    }

    pub(crate) fn StateFrmForall(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(variables)] => {
                Ok(variables)
            },
        )
    }

    pub(crate) fn StateFrmMu(input: ParseNode) -> ParseResult<StateVarDecl> {
        match_nodes!(input.into_children();
            [StateVarDecl(variable)] => {
                Ok(variable)
            },
        )
    }

    pub(crate) fn StateFrmNu(input: ParseNode) -> ParseResult<StateVarDecl> {
        match_nodes!(input.into_children();
            [StateVarDecl(variable)] => {
                Ok(variable)
            },
        )
    }

    pub(crate) fn StateFrmDelay(input: ParseNode) -> ParseResult<StateFrm> {
        let span: Span = input.as_span().into();
        // The `@`-time argument is optional, so there may be zero or one child.
        match input.into_children().next() {
            Some(child) => Ok(StateFrmKind::Delay(Some(Mcrl2Parser::DataExpr(child)?)).spanned(span)),
            None => Ok(StateFrmKind::Delay(None).spanned(span)),
        }
    }

    pub(crate) fn StateFrmYaled(input: ParseNode) -> ParseResult<StateFrm> {
        let span: Span = input.as_span().into();
        // The `@`-time argument is optional, so there may be zero or one child.
        match input.into_children().next() {
            Some(child) => Ok(StateFrmKind::Yaled(Some(Mcrl2Parser::DataExpr(child)?)).spanned(span)),
            None => Ok(StateFrmKind::Yaled(None).spanned(span)),
        }
    }

    pub(crate) fn StateFrmNegation(input: ParseNode) -> ParseResult<StateFrm> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [StateFrm(state)] => {
                Ok(StateFrmKind::Unary { op: crate::StateFrmUnaryOp::Negation, expr: Box::new(state) }.spanned(span))
            },
        )
    }

    pub(crate) fn StateFrmLeftConstantMultiply(input: ParseNode) -> ParseResult<DataExpr> {
        match_nodes!(input.into_children();
            [DataValExpr(expr)] => {
                Ok(expr)
            },
        )
    }

    pub(crate) fn StateFrmRightConstantMultiply(input: ParseNode) -> ParseResult<DataExpr> {
        match_nodes!(input.into_children();
            [DataValExpr(expr)] => {
                Ok(expr)
            },
        )
    }

    pub(crate) fn StateFrmDiamond(input: ParseNode) -> ParseResult<RegFrm> {
        match_nodes!(input.into_children();
            [RegFrm(formula)] => {
                Ok(formula)
            },
        )
    }

    pub(crate) fn StateFrmBox(input: ParseNode) -> ParseResult<RegFrm> {
        match_nodes!(input.into_children();
            [RegFrm(formula)] => {
                Ok(formula)
            },
        )
    }

    pub(crate) fn StateFrmSup(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(variables)] => {
                Ok(variables)
            },
        )
    }

    pub(crate) fn StateFrmInf(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(variables)] => {
                Ok(variables)
            },
        )
    }

    pub(crate) fn StateFrmSum(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(variables)] => {
                Ok(variables)
            },
        )
    }

    pub(crate) fn StateFrm(input: ParseNode) -> ParseResult<StateFrm> {
        parse_statefrm(input.children().as_pairs().clone())
    }

    fn StateVarDecl(input: ParseNode) -> ParseResult<StateVarDecl> {
        let span = input.as_span();
        match_nodes!(input.into_children();
            [Id(identifier), StateVarAssignmentList(arguments)] => {
                Ok(StateVarDecl {
                    identifier: identifier.node,
                    arguments,
                    span: span.into(),
                    id: None,
                })
            },
            [Id(identifier)] => {
                Ok(StateVarDecl {
                    identifier: identifier.node,
                    arguments: Vec::new(),
                    span: span.into(),
                    id: None,
                })
            }
        )
    }

    fn StateVarAssignmentList(input: ParseNode) -> ParseResult<Vec<StateVarAssignment>> {
        match_nodes!(input.into_children();
            [StateVarAssignment(assignments)..] => {
                Ok(assignments.collect())
            }
        )
    }

    fn StateVarAssignment(input: ParseNode) -> ParseResult<StateVarAssignment> {
        match_nodes!(input.into_children();
            [Id(identifier), SortExpr(sort), DataExpr(expr)] => {
                Ok(StateVarAssignment { identifier, sort, expr, id: None })
            }
        )
    }
}
