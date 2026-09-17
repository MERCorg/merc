use crate::Mcrl2Parser;
use crate::RegFrm;
use crate::Rule;
use crate::parse_regfrm;

use super::ParseNode;
use super::ParseResult;

#[merc_pest_consume::parser_methods]
impl Mcrl2Parser {
    pub(crate) fn RegFrm(input: ParseNode) -> ParseResult<RegFrm> {
        parse_regfrm(input.children().as_pairs().clone())
    }
}
