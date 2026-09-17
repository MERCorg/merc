use merc_pest_consume::match_nodes;
use merc_utilities::Span;
use std::iter;

use crate::Action;
use crate::ActionName;
use crate::CommExpr;
use crate::DataExpr;
use crate::IdDecl;
use crate::Mcrl2Parser;
use crate::MultiAction;
use crate::MultiActionLabel;
use crate::ProcessExpr;
use crate::ProcessExprKind;
use crate::Rename;
use crate::Rule;
use crate::parse_process_expr;

use super::ParseNode;
use super::ParseResult;

#[merc_pest_consume::parser_methods]
impl Mcrl2Parser {
    pub(crate) fn ProcExprAt(input: ParseNode) -> ParseResult<DataExpr> {
        match_nodes!(input.into_children();
            [DataExprUnit(expr)] => {
                Ok(expr)
            },
        )
    }

    pub(crate) fn ActIdSet(actions: ParseNode) -> ParseResult<Vec<ActionName>> {
        match_nodes!(actions.into_children();
            [IdList(list)] => {
                Ok(list.into_iter().map(|(node, span)| ActionName { node, span }).collect())
            },
        )
    }

    fn MultActId(actions: ParseNode) -> ParseResult<MultiActionLabel> {
        match_nodes!(actions.into_children();
            [Id(actions)..] => {
                Ok(MultiActionLabel { actions: actions.collect() })
            },
        )
    }

    fn MultActIdList(actions: ParseNode) -> ParseResult<Vec<MultiActionLabel>> {
        match_nodes!(actions.into_children();
            [MultActId(action), MultActId(actions)..] => {
                Ok(iter::once(action).chain(actions).collect())
            },
        )
    }

    pub(crate) fn MultActIdSet(actions: ParseNode) -> ParseResult<Vec<MultiActionLabel>> {
        match_nodes!(actions.into_children();
            [MultActIdList(list)] => {
                Ok(list)
            },
        )
    }

    pub(crate) fn ProcExpr(input: ParseNode) -> ParseResult<ProcessExpr> {
        parse_process_expr(input.children().as_pairs().clone())
    }

    fn ProcExprNoIf(input: ParseNode) -> ParseResult<ProcessExpr> {
        parse_process_expr(input.children().as_pairs().clone())
    }

    pub(crate) fn ProcExprId(input: ParseNode) -> ParseResult<ProcessExpr> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [Id(identifier)] => {
                Ok(ProcessExprKind::Id(identifier, Vec::new()).spanned(span))
            },
            [Id(identifier), AssignmentList(assignments)] => {
                Ok(ProcessExprKind::Id(identifier, assignments).spanned(span))
            },
        )
    }

    pub(crate) fn ProcExprBlock(input: ParseNode) -> ParseResult<ProcessExpr> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [ActIdSet(actions), ProcExpr(expr)] => {
                Ok(ProcessExprKind::Block {
                    actions,
                    operand: Box::new(expr),
                }.spanned(span))
            },
        )
    }

    pub(crate) fn ProcExprIf(input: ParseNode) -> ParseResult<DataExpr> {
        match_nodes!(input.into_children();
            [DataExpr(condition)] => {
                Ok(condition)
            },
        )
    }

    pub(crate) fn ProcExprIfThen(input: ParseNode) -> ParseResult<(DataExpr, ProcessExpr)> {
        match_nodes!(input.into_children();
            [DataExpr(condition), ProcExprNoIf(expr)] => {
                Ok((condition, expr))
            },
        )
    }

    pub(crate) fn ProcExprAllow(input: ParseNode) -> ParseResult<ProcessExpr> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [MultActIdSet(actions), ProcExpr(expr)] => {
                Ok(ProcessExprKind::Allow {
                    actions,
                    operand: Box::new(expr),
                }.spanned(span))
            },
        )
    }

    pub(crate) fn ProcExprHide(input: ParseNode) -> ParseResult<ProcessExpr> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [ActIdSet(actions), ProcExpr(expr)] => {
                Ok(ProcessExprKind::Hide {
                    actions,
                    operand: Box::new(expr),
                }.spanned(span))
            },
        )
    }

    fn ActionList(actions: ParseNode) -> ParseResult<Vec<Action>> {
        match_nodes!(actions.into_children();
            [Action(action), Action(actions)..] => {
                Ok(iter::once(action).chain(actions).collect())
            },
        )
    }

    pub(crate) fn MultiActTau(_input: ParseNode) -> ParseResult<()> {
        Ok(())
    }

    pub(crate) fn ProcExprDelta(_input: ParseNode) -> ParseResult<()> {
        Ok(())
    }

    pub(crate) fn MultAct(input: ParseNode) -> ParseResult<MultiAction> {
        match_nodes!(input.into_children();
            [MultiActTau(_)] => {
                Ok(MultiAction { actions: Vec::new() })
            },
            [ActionList(actions)] => {
                Ok(MultiAction { actions })
            },
        )
    }

    fn CommExpr(action: ParseNode) -> ParseResult<CommExpr> {
        match_nodes!(action.into_children();
            [Id(first), MultActId(mut multiact), Id(to)] => {
                multiact.actions.insert(0, first);
                Ok(CommExpr { from: multiact, to })
            },
        )
    }

    fn CommExprList(actions: ParseNode) -> ParseResult<Vec<CommExpr>> {
        match_nodes!(actions.into_children();
            [CommExpr(action), CommExpr(actions)..] => {
                Ok(iter::once(action).chain(actions).collect())
            },
        )
    }

    pub(crate) fn CommExprSet(actions: ParseNode) -> ParseResult<Vec<CommExpr>> {
        match_nodes!(actions.into_children();
            [CommExprList(list)] => {
                Ok(list)
            },
        )
    }

    pub(crate) fn ProcExprRename(input: ParseNode) -> ParseResult<ProcessExpr> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [RenExprSet(renames), ProcExpr(expr)] => {
                Ok(ProcessExprKind::Rename {
                    renames,
                    operand: Box::new(expr),
                }.spanned(span))
            },
        )
    }

    pub(crate) fn ProcExprComm(input: ParseNode) -> ParseResult<ProcessExpr> {
        let span: Span = input.as_span().into();
        match_nodes!(input.into_children();
            [CommExprSet(comm), ProcExpr(expr)] => {
                Ok(ProcessExprKind::Comm {
                    comm,
                    operand: Box::new(expr),
                }.spanned(span))
            },
        )
    }

    pub(crate) fn Action(input: ParseNode) -> ParseResult<Action> {
        match_nodes!(input.into_children();
            [Id(id)] => {
                Ok(Action { id, args: Vec::new() })
            },
            [Id(id), DataExprList(args)] => {
                Ok(Action { id, args })
            },
        )
    }

    fn RenExprSet(renames: ParseNode) -> ParseResult<Vec<Rename>> {
        match_nodes!(renames.into_children();
            [RenExprList(renames)] => {
                Ok(renames)
            },
        )
    }

    fn RenExprList(renames: ParseNode) -> ParseResult<Vec<Rename>> {
        match_nodes!(renames.into_children();
            [RenExpr(renames)..] => {
                Ok(renames.collect())
            },
        )
    }

    fn RenExpr(renames: ParseNode) -> ParseResult<Rename> {
        match_nodes!(renames.into_children();
            [Id(from), Id(to)] => {
                Ok(Rename { from, to })
            },
        )
    }

    pub(crate) fn ProcExprSum(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(variables)] => {
                Ok(variables)
            },
        )
    }

    pub(crate) fn ProcExprDist(input: ParseNode) -> ParseResult<(Vec<IdDecl>, DataExpr)> {
        match_nodes!(input.into_children();
            [VarsDeclList(variables), DataExpr(expr)] => {
                Ok((variables, expr))
            },
        )
    }
}
