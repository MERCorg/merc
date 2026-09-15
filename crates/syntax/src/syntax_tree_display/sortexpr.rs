use std::fmt;

use itertools::Itertools;

use crate::ComplexSort;
use crate::ConstructorDecl;
use crate::Sort;
use crate::SortExpression;
use crate::SortExpressionKind;

impl fmt::Display for Sort {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl fmt::Display for ComplexSort {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl fmt::Display for SortExpression {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.node {
            SortExpressionKind::Product { lhs, rhs } => write!(f, "({lhs} # {rhs})"),
            SortExpressionKind::Function { domain, range } => write!(f, "({domain} -> {range})"),
            SortExpressionKind::Reference(name) => write!(f, "{name}"),
            SortExpressionKind::TypeVar(name) => write!(f, "'{name}"),
            SortExpressionKind::ResolvedTypeVar(id) => write!(f, "'{id}"),
            SortExpressionKind::Simple(sort) => write!(f, "{sort}"),
            SortExpressionKind::Complex(complex, inner) => write!(f, "{complex}({inner})"),
            SortExpressionKind::Struct { inner } => {
                write!(f, "struct ")?;
                write!(f, "{}", inner.iter().format(" | "))
            }
            SortExpressionKind::Resolved(name, _id) => write!(f, "{name}"),
            SortExpressionKind::FlattenedFunction { domain, range } => {
                let domain = domain.iter().format(" # ");
                write!(f, "({domain} -> {range})")
            }
        }
    }
}

impl fmt::Display for ConstructorDecl {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.args.is_empty() {
            write!(f, "{}", self.name.node)?;

            if let Some(projection) = &self.projection {
                write!(f, "?{}", projection.node)?;
            }

            Ok(())
        } else {
            write!(f, "{}(", self.name.node)?;
            for (i, (name, sort)) in self.args.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                match name {
                    Some(name) => write!(f, "{} : {sort}", name.node)?,
                    None => write!(f, "{sort}")?,
                }
            }
            write!(f, ")")?;

            if let Some(projection) = &self.projection {
                write!(f, "?{}", projection.node)?;
            }

            Ok(())
        }
    }
}
