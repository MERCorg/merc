use std::fmt;

use crate::RegFrm;
use crate::RegFrmKind;

impl fmt::Display for RegFrm {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.node {
            RegFrmKind::Action(action) => write!(f, "{action}"),
            RegFrmKind::Iteration(body) => write!(f, "({body})*"),
            RegFrmKind::Plus(body) => write!(f, "({body})+"),
            RegFrmKind::Choice { lhs, rhs } => write!(f, "({lhs} + {rhs})"),
            RegFrmKind::Sequence { lhs, rhs } => write!(f, "({lhs} . {rhs})"),
        }
    }
}
