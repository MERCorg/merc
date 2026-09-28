use rand::Rng;
use rand::prelude::IteratorRandom;

use crate::DataExpr;
use crate::DataExprBinaryOp;
use crate::DataExprKind;
use crate::IdDecl;
use crate::Sort;
use crate::SortExpressionKind;

/// Builds a spanless identifier expression.
fn id(identifier: String) -> DataExpr {
    DataExprKind::Id(identifier).into()
}

/// Builds a spanless number literal expression.
fn number(value: &str) -> DataExpr {
    DataExprKind::Number(value.to_string()).into()
}

/// Builds a spanless binary expression.
fn binary(op: DataExprBinaryOp, lhs: DataExpr, rhs: DataExpr) -> DataExpr {
    DataExprKind::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
    .into()
}

/// Generates a random boolean data expression from the given variable list.
pub fn random_boolean_data_expression<R: Rng, Id>(rng: &mut R, variables: &[IdDecl<Id>]) -> DataExpr {
    let integers: Vec<&IdDecl<Id>> = variables
        .iter()
        .filter(
            |v| matches!(&v.sort.node, SortExpressionKind::Simple(s) if matches!(s, Sort::Int | Sort::Nat | Sort::Pos)),
        )
        .collect();
    let booleans: Vec<&IdDecl<Id>> = variables
        .iter()
        .filter(|v| matches!(&v.sort.node, SortExpressionKind::Simple(Sort::Bool)))
        .collect();

    let mut candidates: Vec<DataExpr> = booleans.iter().map(|v| id(v.identifier.node.clone())).collect();

    for m in &integers {
        let mv = id(m.identifier.node.clone());
        candidates.push(binary(DataExprBinaryOp::GreaterThan, mv.clone(), number("0")));
        candidates.push(binary(DataExprBinaryOp::GreaterThan, mv.clone(), number("1")));
        candidates.push(binary(DataExprBinaryOp::LessThan, mv.clone(), number("2")));
        candidates.push(binary(DataExprBinaryOp::LessThan, mv.clone(), number("3")));
        for n in &integers {
            candidates.push(binary(
                DataExprBinaryOp::Equal,
                mv.clone(),
                id(n.identifier.node.clone()),
            ));
        }
    }

    candidates.push(DataExprKind::Bool(true).into());
    candidates.push(DataExprKind::Bool(false).into());

    candidates.into_iter().choose(rng).unwrap()
}

/// Generates a random integer data expression from the given variable list.
pub fn random_integer_data_expression<R: Rng, Id>(rng: &mut R, variables: &[IdDecl<Id>]) -> DataExpr {
    let integers: Vec<&IdDecl<Id>> = variables
        .iter()
        .filter(
            |v| matches!(&v.sort.node, SortExpressionKind::Simple(s) if matches!(s, Sort::Int | Sort::Nat | Sort::Pos)),
        )
        .collect();

    let extras = [number("1"), number("2")];
    let rhs_operands: Vec<DataExpr> = integers
        .iter()
        .map(|v| id(v.identifier.node.clone()))
        .chain(extras)
        .collect();

    let mut candidates: Vec<DataExpr> = integers.iter().map(|v| id(v.identifier.node.clone())).collect();

    for m in &integers {
        let mv = id(m.identifier.node.clone());
        for n in &rhs_operands {
            candidates.push(binary(DataExprBinaryOp::Add, mv.clone(), n.clone()));
            candidates.push(binary(DataExprBinaryOp::Subtract, mv.clone(), n.clone()));
        }
    }

    candidates.push(number("0"));
    candidates.push(number("1"));

    candidates.into_iter().choose(rng).unwrap()
}
