use itertools::Itertools;
use merc_pest_consume::match_nodes;
use merc_utilities::Span;

use crate::ComplexSort;
use crate::ConstructorDecl;
use crate::Mcrl2Parser;
use crate::Rule;
use crate::SortExpression;
use crate::SortExpressionKind;
use crate::Spanned;
use crate::parse_sortexpr;
use crate::parse_sortexpr_primary;

use super::ParseNode;
use super::ParseResult;

#[merc_pest_consume::parser_methods]
impl Mcrl2Parser {
    fn SortExprPrimary(sort: ParseNode) -> ParseResult<SortExpression> {
        parse_sortexpr(sort.children().as_pairs().clone())
    }

    pub(crate) fn SortExpr(expr: ParseNode) -> ParseResult<SortExpression> {
        parse_sortexpr(expr.children().as_pairs().clone())
    }

    // Complex sorts
    pub(crate) fn SortExprList(inner: ParseNode) -> ParseResult<SortExpression> {
        let span: Span = inner.as_span().into();
        Ok(SortExpressionKind::Complex(
            ComplexSort::List,
            Box::new(parse_sortexpr(inner.children().as_pairs().clone())?),
        )
        .spanned(span))
    }

    pub(crate) fn SortExprSet(inner: ParseNode) -> ParseResult<SortExpression> {
        let span: Span = inner.as_span().into();
        Ok(SortExpressionKind::Complex(
            ComplexSort::Set,
            Box::new(parse_sortexpr(inner.children().as_pairs().clone())?),
        )
        .spanned(span))
    }

    pub(crate) fn SortExprBag(inner: ParseNode) -> ParseResult<SortExpression> {
        let span: Span = inner.as_span().into();
        Ok(SortExpressionKind::Complex(
            ComplexSort::Bag,
            Box::new(parse_sortexpr(inner.children().as_pairs().clone())?),
        )
        .spanned(span))
    }

    pub(crate) fn SortExprFSet(inner: ParseNode) -> ParseResult<SortExpression> {
        let span: Span = inner.as_span().into();
        Ok(SortExpressionKind::Complex(
            ComplexSort::FSet,
            Box::new(parse_sortexpr(inner.children().as_pairs().clone())?),
        )
        .spanned(span))
    }

    pub(crate) fn SortExprFBag(inner: ParseNode) -> ParseResult<SortExpression> {
        let span: Span = inner.as_span().into();
        Ok(SortExpressionKind::Complex(
            ComplexSort::FBag,
            Box::new(parse_sortexpr(inner.children().as_pairs().clone())?),
        )
        .spanned(span))
    }

    pub(crate) fn SortExprStruct(inner: ParseNode) -> ParseResult<SortExpression> {
        let span: Span = inner.as_span().into();
        match_nodes!(inner.into_children();
            [ConstrDeclList(inner)] => {
                Ok(SortExpressionKind::Struct { inner }.spanned(span))
            },
        )
    }

    pub(crate) fn ConstrDeclList(input: ParseNode) -> ParseResult<Vec<ConstructorDecl>> {
        match_nodes!(input.into_children();
            [ConstrDecl(decl)..] => {
                Ok(decl.collect())
            },
        )
    }

    // `ConstrDecl = { IdAt ~ ( "(" ~ ProjDeclList ~ ")" )? ~ ( "?" ~ IdAt )? }`: one arm per
    // combination of the two optional groups. The leading name and the trailing recogniser each
    // keep their own span.
    pub(crate) fn ConstrDecl(input: ParseNode) -> ParseResult<ConstructorDecl> {
        match_nodes!(input.into_children();
            [IdAt(name)] => {
                Ok(ConstructorDecl { name, args: Vec::new(), projection: None })
            },
            [IdAt(name), ProjDeclList(args)] => {
                Ok(ConstructorDecl { name, args, projection: None })
            },
            [IdAt(name), IdAt(projection)] => {
                Ok(ConstructorDecl { name, args: Vec::new(), projection: Some(projection) })
            },
            [IdAt(name), ProjDeclList(args), IdAt(projection)] => {
                Ok(ConstructorDecl { name, args, projection: Some(projection) })
            },
        )
    }

    pub(crate) fn ProjDeclList(input: ParseNode) -> ParseResult<Vec<(Option<Spanned<String>>, SortExpression)>> {
        match_nodes!(input.into_children();
            [ProjDecl(decl)..] => {
                Ok(decl.collect())
            },
        )
    }

    pub(crate) fn ProjDecl(input: ParseNode) -> ParseResult<(Option<Spanned<String>>, SortExpression)> {
        match_nodes!(input.into_children();
            [SortExpr(sort)] => {
                Ok((None, sort))
            },
            [Id(name), SortExpr(sort)] => {
                Ok((Some(name), sort))
            },
        )
    }

    pub(crate) fn SortProduct(sort: ParseNode) -> ParseResult<Vec<SortExpression>> {
        let mut iter = sort.into_children();

        // An expression of the shape SortExprPrimary ~ (SortExprProduct ~ SortExprPrimary)*
        let mut result = vec![parse_sortexpr_primary(iter.next().unwrap().as_pair().clone())?];

        for mut chunk in &iter.chunks(2) {
            if chunk.next().unwrap().as_rule() == Rule::SortExprProduct {
                let sort = parse_sortexpr_primary(chunk.next().unwrap().as_pair().clone())?;
                result.push(sort);
            }
        }

        Ok(result)
    }
}
