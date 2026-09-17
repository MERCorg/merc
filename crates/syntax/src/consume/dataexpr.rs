use merc_pest_consume::match_nodes;
use merc_utilities::Span;

use crate::Assignment;
use crate::AssignmentData;
use crate::BagElement;
use crate::DataExpr;
use crate::DataExprKind;
use crate::DataExprUnaryOp;
use crate::DataExprUpdate;
use crate::IdDecl;
use crate::Mcrl2Parser;
use crate::Rule;
use crate::parse_dataexpr;

use super::ParseNode;
use super::ParseResult;

#[merc_pest_consume::parser_methods]
impl Mcrl2Parser {
    pub(crate) fn DataExpr(expr: ParseNode) -> ParseResult<DataExpr> {
        parse_dataexpr(expr.children().as_pairs().clone())
    }

    pub(crate) fn DataExprUnit(expr: ParseNode) -> ParseResult<DataExpr> {
        parse_dataexpr(expr.children().as_pairs().clone())
    }

    pub(crate) fn DataValExpr(expr: ParseNode) -> ParseResult<DataExpr> {
        match_nodes!(expr.into_children();
            [DataExpr(expr)] => {
                Ok(expr)
            },
        )
    }

    pub(crate) fn DataExprUpdate(expr: ParseNode) -> ParseResult<DataExprUpdate> {
        match_nodes!(expr.into_children();
            [DataExpr(expr), DataExpr(update)] => {
                Ok(DataExprUpdate { expr, update })
            },
        )
    }

    pub(crate) fn DataExprApplication(expr: ParseNode) -> ParseResult<Vec<DataExpr>> {
        match_nodes!(expr.into_children();
            [DataExprList(expressions)] => {
                Ok(expressions)
            },
        )
    }

    pub(crate) fn DataExprWhr(expr: ParseNode) -> ParseResult<Vec<Assignment>> {
        match_nodes!(expr.into_children();
            [AssignmentList(assignments)] => {
                Ok(assignments)
            },
        )
    }

    pub(crate) fn AssignmentList(assignments: ParseNode) -> ParseResult<Vec<Assignment>> {
        match_nodes!(assignments.into_children();
            [Assignment(assignment)] => {
                Ok(vec![assignment])
            },
            [Assignment(assignment)..] => {
                Ok(assignment.collect())
            },
        )
    }

    pub(crate) fn Assignment(assignment: ParseNode) -> ParseResult<Assignment> {
        match_nodes!(assignment.into_children();
            [IdAt(identifier), DataExpr(expr)] => {
                Ok(AssignmentData { identifier: identifier.node, expr, id: None }.spanned(identifier.span))
            },
        )
    }

    pub(crate) fn DataExprSize(expr: ParseNode) -> ParseResult<DataExpr> {
        let span: Span = expr.as_span().into();
        match_nodes!(expr.into_children();
            [DataExpr(expr)] => {
                Ok(DataExprKind::Unary { op: DataExprUnaryOp::Size, expr: Box::new(expr) }.spanned(span))
            },
        )
    }

    pub(crate) fn DataExprListEnum(input: ParseNode) -> ParseResult<DataExpr> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [DataExprList(expressions)] => {
                Ok(DataExprKind::List(expressions).spanned(span))
            },
        )
    }

    pub(crate) fn DataExprBagEnum(input: ParseNode) -> ParseResult<DataExpr> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [BagEnumEltList(elements)] => {
                Ok(DataExprKind::Bag(elements).spanned(span))
            },
        )
    }

    fn BagEnumEltList(input: ParseNode) -> ParseResult<Vec<BagElement>> {
        match_nodes!(input.into_children();
            [BagEnumElt(elements)..] => {
                Ok(elements.collect())
            },
        )
    }

    fn BagEnumElt(input: ParseNode) -> ParseResult<BagElement> {
        match_nodes!(input.into_children();
            [DataExpr(expr), DataExpr(multiplicity)] => {
                Ok(BagElement { expr, multiplicity })
            },
        )
    }

    pub(crate) fn DataExprSetEnum(input: ParseNode) -> ParseResult<DataExpr> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [DataExprList(expressions)] => {
                Ok(DataExprKind::Set(expressions).spanned(span))
            },
        )
    }

    pub(crate) fn DataExprSetBagComp(input: ParseNode) -> ParseResult<DataExpr> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [VarDecl(variable), DataExpr(predicate)] => {
                Ok(DataExprKind::SetBagComp { variable, predicate: Box::new(predicate) }.spanned(span))
            },
        )
    }

    pub(crate) fn Number(input: ParseNode) -> ParseResult<DataExpr> {
        let span: Span = input.as_span().into();
        Ok(DataExprKind::Number(input.as_str().into()).spanned(span))
    }

    fn VarDecl(decl: ParseNode) -> ParseResult<IdDecl> {
        match_nodes!(decl.into_children();
            [IdAt(identifier), SortExpr(sort)] => {
                Ok(IdDecl::new(identifier.node, sort, identifier.span))
            },
        )
    }

    pub(crate) fn DataExprLambda(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(vars)] => {
                Ok(vars)
            },
        )
    }

    pub(crate) fn DataExprForall(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(vars)] => {
                Ok(vars)
            },
        )
    }

    pub(crate) fn DataExprExists(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(vars)] => {
                Ok(vars)
            },
        )
    }
}
