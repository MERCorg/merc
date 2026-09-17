use merc_pest_consume::match_nodes;

use crate::ActFrm;
use crate::IdDecl;
use crate::Mcrl2Parser;
use crate::Rule;
use crate::parse_actfrm;

use super::ParseNode;
use super::ParseResult;

#[merc_pest_consume::parser_methods]
impl Mcrl2Parser {
    pub(crate) fn ActFrmExists(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(variables)] => {
                Ok(variables)
            },
        )
    }

    pub(crate) fn ActFrmForall(input: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(input.into_children();
            [VarsDeclList(variables)] => {
                Ok(variables)
            },
        )
    }

    pub(crate) fn ActFrm(input: ParseNode) -> ParseResult<ActFrm> {
        parse_actfrm(input.children().as_pairs().clone())
    }
}
