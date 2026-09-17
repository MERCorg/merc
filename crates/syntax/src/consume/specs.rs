use merc_pest_consume::Error;
use merc_pest_consume::match_nodes;
use merc_utilities::Span;
use pest::error::ErrorVariant;

use crate::ActDecl;
use crate::ActionName;
use crate::ActionRHS;
use crate::ActionRenameDecl;
use crate::ActionRenameRule;
use crate::ConstructorId;
use crate::EqnDecl;
use crate::EqnSpec;
use crate::EqnSpecData;
use crate::FixedPointOperator;
use crate::IdDecl;
use crate::MapId;
use crate::Mcrl2Parser;
use crate::PbesEquation;
use crate::PresEquation;
use crate::ProcDecl;
use crate::ProcessExpr;
use crate::PropVarDecl;
use crate::PropVarInst;
use crate::PropVarInstData;
use crate::Rule;
use crate::SortDecl;
use crate::StateFrm;
use crate::TypeVarDecl;
use crate::UntypedActionRenameSpec;
use crate::UntypedDataSpecification;
use crate::UntypedPbes;
use crate::UntypedPres;
use crate::UntypedProcessSpecification;
use crate::UntypedStateFrmSpec;

use super::ParseNode;
use super::ParseResult;

/// Declarations shared by every `UntypedDataSpecification`-bearing top-level spec
/// (`MCRL2Spec`, `ActionRenameSpec`, `StateFrmSpec`), collected while walking that spec's own
/// top-level declaration rules.
#[derive(Default)]
struct DataSpecDeclarations {
    map_declarations: Vec<IdDecl<MapId>>,
    constructor_declarations: Vec<IdDecl<ConstructorId>>,
    equation_declarations: Vec<EqnSpec>,
    sort_declarations: Vec<SortDecl>,
    type_var_declarations: Vec<TypeVarDecl>,
}

impl DataSpecDeclarations {
    fn into_data_specification(self) -> UntypedDataSpecification {
        UntypedDataSpecification {
            map_declarations: self.map_declarations,
            constructor_declarations: self.constructor_declarations,
            equation_declarations: self.equation_declarations,
            sort_declarations: self.sort_declarations,
            type_var_declarations: self.type_var_declarations,
        }
    }
}

struct ProcessSpecDeclarations {
    data: DataSpecDeclarations,
    action_declarations: Vec<ActDecl>,
    global_variables: Vec<IdDecl>,
    process_declarations: Vec<ProcDecl>,
    init: Option<ProcessExpr>,
}

/// Walks `MCRL2Spec`'s own top-level declaration rules, collecting each into its matching field.
fn collect_process_spec(spec: ParseNode) -> ParseResult<ProcessSpecDeclarations> {
    let mut decls = ProcessSpecDeclarations {
        data: DataSpecDeclarations::default(),
        action_declarations: Vec::new(),
        global_variables: Vec::new(),
        process_declarations: Vec::new(),
        init: None,
    };

    for child in spec.into_children() {
        match child.as_rule() {
            Rule::ActSpec => {
                decls.action_declarations.extend(Mcrl2Parser::ActSpec(child)?);
            }
            Rule::ConsSpec => {
                decls
                    .data
                    .constructor_declarations
                    .append(&mut Mcrl2Parser::ConsSpec(child)?);
            }
            Rule::MapSpec => {
                decls.data.map_declarations.append(&mut Mcrl2Parser::MapSpec(child)?);
            }
            Rule::GlobVarSpec => {
                decls.global_variables.append(&mut Mcrl2Parser::GlobVarSpec(child)?);
            }
            Rule::EqnSpec => {
                decls
                    .data
                    .equation_declarations
                    .append(&mut Mcrl2Parser::EqnSpec(child)?);
            }
            Rule::ProcSpec => {
                decls.process_declarations.append(&mut Mcrl2Parser::ProcSpec(child)?);
            }
            Rule::SortSpec => {
                decls.data.sort_declarations.append(&mut Mcrl2Parser::SortSpec(child)?);
            }
            Rule::TypeVarSpec => {
                decls
                    .data
                    .type_var_declarations
                    .append(&mut Mcrl2Parser::TypeVarSpec(child)?);
            }
            Rule::Init => {
                if decls.init.is_some() {
                    return Err(Error::new_from_span(
                        ErrorVariant::CustomError {
                            message: "Multiple init expressions are not allowed".to_string(),
                        },
                        child.as_span(),
                    ));
                }

                decls.init = Some(Mcrl2Parser::Init(child)?);
            }
            Rule::EOI => {
                // End of input
                break;
            }
            _ => {
                unimplemented!("Unexpected rule: {:?}", child.as_rule());
            }
        }
    }

    Ok(decls)
}

struct ActionRenameSpecDeclarations {
    data: DataSpecDeclarations,
    action_declarations: Vec<ActDecl>,
    rename_declarations: Vec<ActionRenameDecl>,
}

/// Walks `ActionRenameSpec`'s own top-level declaration rules, collecting each into its matching
/// field.
fn collect_action_rename_spec(spec: ParseNode) -> ParseResult<ActionRenameSpecDeclarations> {
    let mut decls = ActionRenameSpecDeclarations {
        data: DataSpecDeclarations::default(),
        action_declarations: Vec::new(),
        rename_declarations: Vec::new(),
    };

    for child in spec.into_children() {
        match child.as_rule() {
            Rule::ConsSpec => {
                decls
                    .data
                    .constructor_declarations
                    .append(&mut Mcrl2Parser::ConsSpec(child)?);
            }
            Rule::MapSpec => {
                decls.data.map_declarations.append(&mut Mcrl2Parser::MapSpec(child)?);
            }
            Rule::EqnSpec => {
                decls
                    .data
                    .equation_declarations
                    .append(&mut Mcrl2Parser::EqnSpec(child)?);
            }
            Rule::SortSpec => {
                decls.data.sort_declarations.append(&mut Mcrl2Parser::SortSpec(child)?);
            }
            Rule::TypeVarSpec => {
                decls
                    .data
                    .type_var_declarations
                    .append(&mut Mcrl2Parser::TypeVarSpec(child)?);
            }
            Rule::ActSpec => {
                decls.action_declarations.append(&mut Mcrl2Parser::ActSpec(child)?);
            }
            Rule::ActionRenameRuleSpec => {
                decls
                    .rename_declarations
                    .append(&mut Mcrl2Parser::ActionRenameRuleSpec(child)?);
            }
            Rule::EOI => {
                // End of input
                break;
            }
            _ => {
                unimplemented!("Unexpected rule: {:?}", child.as_rule());
            }
        }
    }

    Ok(decls)
}

struct StateFrmSpecDeclarations {
    data: DataSpecDeclarations,
    action_declarations: Vec<ActDecl>,
    formula: Option<StateFrm>,
}

/// Consumes a single `StateFrmSpecElt` child (already unwrapped to its one inner rule),
/// collecting it into its matching field.
fn collect_state_frm_spec_elt(element: ParseNode, decls: &mut StateFrmSpecDeclarations) -> ParseResult<()> {
    match element.as_rule() {
        Rule::ConsSpec => {
            decls
                .data
                .constructor_declarations
                .append(&mut Mcrl2Parser::ConsSpec(element)?);
        }
        Rule::MapSpec => {
            decls.data.map_declarations.append(&mut Mcrl2Parser::MapSpec(element)?);
        }
        Rule::EqnSpec => {
            decls
                .data
                .equation_declarations
                .append(&mut Mcrl2Parser::EqnSpec(element)?);
        }
        Rule::SortSpec => {
            decls
                .data
                .sort_declarations
                .append(&mut Mcrl2Parser::SortSpec(element)?);
        }
        Rule::TypeVarSpec => {
            decls
                .data
                .type_var_declarations
                .append(&mut Mcrl2Parser::TypeVarSpec(element)?);
        }
        Rule::ActSpec => {
            decls.action_declarations.append(&mut Mcrl2Parser::ActSpec(element)?);
        }
        _ => {
            unimplemented!("Unexpected rule in StateFrmSpecElt: {:?}", element.as_rule());
        }
    }
    Ok(())
}

/// Walks `StateFrmSpec`'s own top-level declaration rules, collecting each into its matching
/// field.
fn collect_state_frm_spec(spec: ParseNode) -> ParseResult<StateFrmSpecDeclarations> {
    let mut decls = StateFrmSpecDeclarations {
        data: DataSpecDeclarations::default(),
        action_declarations: Vec::new(),
        formula: None,
    };

    for child in spec.into_children() {
        match child.as_rule() {
            Rule::StateFrmSpecElt => {
                let element = child
                    .into_children()
                    .next()
                    .expect("StateFrmSpecElt has exactly one child");
                collect_state_frm_spec_elt(element, &mut decls)?;
            }
            Rule::StateFrm => {
                if decls.formula.is_some() {
                    return Err(Error::new_from_span(
                        ErrorVariant::CustomError {
                            message: "Multiple state formula specifications are not allowed".to_string(),
                        },
                        child.as_span(),
                    ));
                }
                decls.formula = Some(Mcrl2Parser::StateFrm(child)?);
            }
            Rule::FormSpec => {
                if decls.formula.is_some() {
                    return Err(Error::new_from_span(
                        ErrorVariant::CustomError {
                            message: "Multiple state formula specifications are not allowed".to_string(),
                        },
                        child.as_span(),
                    ));
                }
                decls.formula = Some(Mcrl2Parser::FormSpec(child)?);
            }
            Rule::EOI => {
                // End of input
                break;
            }
            _ => {
                unimplemented!("Unexpected rule: {:?}", child.as_rule());
            }
        }
    }

    Ok(decls)
}

#[merc_pest_consume::parser_methods]
impl Mcrl2Parser {
    // Although these are not public, they are the main entry points for consuming the parse tree.
    pub(crate) fn MCRL2Spec(spec: ParseNode) -> ParseResult<UntypedProcessSpecification> {
        let decls = collect_process_spec(spec)?;
        Ok(UntypedProcessSpecification {
            data_specification: decls.data.into_data_specification(),
            global_variables: decls.global_variables,
            action_declarations: decls.action_declarations,
            process_declarations: decls.process_declarations,
            init: decls.init,
        })
    }

    pub fn PbesSpec(spec: ParseNode) -> ParseResult<UntypedPbes> {
        let mut data_specification = None;
        let mut global_variables = None;
        let mut equations = None;
        let mut init = None;

        let span = spec.as_span();
        for child in spec.into_children() {
            match child.as_rule() {
                Rule::DataSpecBody => {
                    data_specification = Some(Mcrl2Parser::DataSpecBody(child)?);
                }
                Rule::GlobVarSpec => {
                    global_variables = Some(Mcrl2Parser::GlobVarSpec(child)?);
                }
                Rule::PbesEqnSpec => {
                    equations = Some(Mcrl2Parser::PbesEqnSpec(child)?);
                }
                Rule::PbesInit => {
                    init = Some(Mcrl2Parser::PbesInit(child)?);
                }
                Rule::EOI => {
                    // End of input
                    break;
                }
                _ => {
                    unimplemented!("Unexpected rule: {:?}", child.as_rule());
                }
            }
        }

        Ok(UntypedPbes {
            data_specification: data_specification.unwrap_or_default(),
            global_variables: global_variables.unwrap_or_default(),
            equations: equations.ok_or_else(|| {
                Error::new_from_span(
                    ErrorVariant::CustomError {
                        message: "A PBES requires a (possibly empty) pbes equation section".to_string(),
                    },
                    span,
                )
            })?,
            init: init.ok_or_else(|| {
                Error::new_from_span(
                    ErrorVariant::CustomError {
                        message: "A PBES requires an init declaration".to_string(),
                    },
                    span,
                )
            })?,
        })
    }

    fn PbesInit(init: ParseNode) -> ParseResult<PropVarInst> {
        match_nodes!(init.into_children();
            [PropVarInst(inst)] => {
                Ok(inst)
            }
        )
    }

    fn PbesEqnSpec(spec: ParseNode) -> ParseResult<Vec<PbesEquation>> {
        match_nodes!(spec.into_children();
            [PbesEqnDecl(equations)..] => {
                Ok(equations.collect())
            },
        )
    }

    fn PbesEqnDecl(decl: ParseNode) -> ParseResult<PbesEquation> {
        let span = decl.as_span();
        match_nodes!(decl.into_children();
            [FixedPointOperator(operator), PropVarDecl(variable), PbesExpr(formula)] => {
                Ok(PbesEquation {
                    operator,
                    variable,
                    formula,
                    span: span.into(),
                })
            },
        )
    }

    fn FixedPointOperator(op: ParseNode) -> ParseResult<FixedPointOperator> {
        match op.into_children().next().unwrap().as_rule() {
            Rule::FixedPointMu => Ok(FixedPointOperator::Least),
            Rule::FixedPointNu => Ok(FixedPointOperator::Greatest),
            x => unimplemented!("This is not a fixed point operator: {:?}", x),
        }
    }

    fn PropVarDecl(decl: ParseNode) -> ParseResult<PropVarDecl> {
        let span = decl.as_span();
        match_nodes!(decl.into_children();
            [Id(identifier), VarsDeclList(params)] => {
                Ok(PropVarDecl {
                    identifier,
                    parameters: params,
                    span: span.into(),
                })
            },
            [Id(identifier)] => {
                let span = identifier.span.clone();
                Ok(PropVarDecl {
                    identifier,
                    parameters: Vec::new(),
                    span,
                })
            }
        )
    }

    pub(crate) fn PropVarInst(inst: ParseNode) -> ParseResult<PropVarInst> {
        let span = inst.as_span();
        match_nodes!(inst.into_children();
            [Id(identifier)] => {
                Ok(PropVarInstData {
                    identifier,
                    arguments: Vec::new(),
                }.spanned(span.into()))
            },
            [Id(identifier), DataExprList(arguments)] => {
                Ok(PropVarInstData {
                    identifier,
                    arguments,
                }.spanned(span.into()))
            }
        )
    }

    pub fn PresSpec(spec: ParseNode) -> ParseResult<UntypedPres> {
        let mut data_specification = None;
        let mut global_variables = None;
        let mut equations = None;
        let mut init = None;

        let span = spec.as_span();
        for child in spec.into_children() {
            match child.as_rule() {
                Rule::DataSpecBody => {
                    data_specification = Some(Mcrl2Parser::DataSpecBody(child)?);
                }
                Rule::GlobVarSpec => {
                    global_variables = Some(Mcrl2Parser::GlobVarSpec(child)?);
                }
                Rule::PresEqnSpec => {
                    equations = Some(Mcrl2Parser::PresEqnSpec(child)?);
                }
                Rule::PbesInit => {
                    init = Some(Mcrl2Parser::PbesInit(child)?);
                }
                Rule::EOI => {
                    // End of input
                    break;
                }
                _ => {
                    unimplemented!("Unexpected rule: {:?}", child.as_rule());
                }
            }
        }

        Ok(UntypedPres {
            data_specification: data_specification.unwrap_or_default(),
            global_variables: global_variables.unwrap_or_default(),
            equations: equations.ok_or_else(|| {
                Error::new_from_span(
                    ErrorVariant::CustomError {
                        message: "A PRES requires a (possibly empty) pres equation section".to_string(),
                    },
                    span,
                )
            })?,
            init: init.ok_or_else(|| {
                Error::new_from_span(
                    ErrorVariant::CustomError {
                        message: "A PRES requires an init declaration".to_string(),
                    },
                    span,
                )
            })?,
        })
    }

    fn PresEqnSpec(spec: ParseNode) -> ParseResult<Vec<PresEquation>> {
        match_nodes!(spec.into_children();
            [PresEqnDecl(equations)..] => {
                Ok(equations.collect())
            },
        )
    }

    fn PresEqnDecl(decl: ParseNode) -> ParseResult<PresEquation> {
        let span = decl.as_span();
        match_nodes!(decl.into_children();
            [FixedPointOperator(operator), PropVarDecl(variable), PresExpr(formula)] => {
                Ok(PresEquation {
                    operator,
                    variable,
                    formula,
                    span: span.into(),
                })
            },
        )
    }

    fn ActSpec(spec: ParseNode) -> ParseResult<Vec<ActDecl>> {
        match_nodes!(spec.into_children();
            [ActDecl(decls)..] => {
                Ok(decls.flatten().collect())
            },
        )
    }

    fn ActDecl(decl: ParseNode) -> ParseResult<Vec<ActDecl>> {
        // Shared by every identifier in the `a, b: Nat` group below: there is no narrower
        // per-name "whole declaration" extent than the group itself.
        let span: Span = decl.as_span().into();
        match_nodes!(decl.into_children();
            [IdList(identifiers)] => {
                Ok(identifiers.into_iter().map(|(name, id_span)| ActDecl {
                    identifier: ActionName { node: name, span: id_span },
                    args: Vec::new(),
                    span: span.clone(),
                }).collect())
            },
            [IdList(identifiers), SortProduct(args)] => {
                Ok(identifiers.into_iter().map(|(name, id_span)| ActDecl {
                    identifier: ActionName { node: name, span: id_span },
                    args: args.clone(),
                    span: span.clone(),
                }).collect())
            },
        )
    }

    fn GlobVarSpec(spec: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(spec.into_children();
            [VarsDeclList(vars)] => {
                Ok(vars)
            }
        )
    }

    pub(crate) fn DataSpec(spec: ParseNode) -> ParseResult<UntypedDataSpecification> {
        // `DataSpecBody` always matches (its repetition allows zero declarations), so it is
        // always the first child, ahead of `EOI`.
        let Some(child) = spec.into_children().next() else {
            return Ok(UntypedDataSpecification::default());
        };

        match child.as_rule() {
            Rule::DataSpecBody => Mcrl2Parser::DataSpecBody(child),
            rule => unimplemented!("Unexpected rule: {:?}", rule),
        }
    }

    pub(crate) fn DataSpecBody(spec: ParseNode) -> ParseResult<UntypedDataSpecification> {
        let mut map_declarations = Vec::new();
        let mut equation_declarations = Vec::new();
        let mut constructor_declarations = Vec::new();
        let mut sort_declarations = Vec::new();
        let mut type_var_declarations = Vec::new();

        for child in spec.into_children() {
            match child.as_rule() {
                Rule::ConsSpec => {
                    constructor_declarations.append(&mut Mcrl2Parser::ConsSpec(child)?);
                }
                Rule::MapSpec => {
                    map_declarations.append(&mut Mcrl2Parser::MapSpec(child)?);
                }
                Rule::EqnSpec => {
                    equation_declarations.append(&mut Mcrl2Parser::EqnSpec(child)?);
                }
                Rule::SortSpec => {
                    sort_declarations.append(&mut Mcrl2Parser::SortSpec(child)?);
                }
                Rule::TypeVarSpec => {
                    type_var_declarations.append(&mut Mcrl2Parser::TypeVarSpec(child)?);
                }
                _ => {
                    unimplemented!("Unexpected rule: {:?}", child.as_rule());
                }
            }
        }

        let data_specification = UntypedDataSpecification {
            map_declarations,
            equation_declarations,
            constructor_declarations,
            sort_declarations,
            type_var_declarations,
        };

        Ok(data_specification)
    }

    pub fn ActionRenameSpec(spec: ParseNode) -> ParseResult<UntypedActionRenameSpec> {
        let decls = collect_action_rename_spec(spec)?;
        Ok(UntypedActionRenameSpec {
            data_specification: decls.data.into_data_specification(),
            action_declarations: decls.action_declarations,
            rename_declarations: decls.rename_declarations,
        })
    }

    fn MapSpec(spec: ParseNode) -> ParseResult<Vec<IdDecl<MapId>>> {
        match_nodes!(spec.into_children();
            [IdsDecl(decls)..] => {
                Ok(decls.flatten().map(IdDecl::retag).collect())
            }
        )
    }

    fn SortSpec(spec: ParseNode) -> ParseResult<Vec<SortDecl>> {
        match_nodes!(spec.into_children();
            [SortDecl(decls)..] => {
                Ok(decls.flatten().collect())
            }
        )
    }

    fn SortDecl(decl: ParseNode) -> ParseResult<Vec<SortDecl>> {
        match_nodes!(decl.into_children();
            // The alias form (`sort A = Bool;`) always names exactly one sort per node.
            [IdAt(identifier), SortExpr(expr)] => {
                Ok(vec![SortDecl::new(identifier.node, Some(expr), identifier.span)])
            },
            // `sort A, B, C;`: each gets its own precise identifier span (see `IdList`).
            [IdList(ids)] => {
                Ok(ids.into_iter().map(|(identifier, span)| SortDecl::new(identifier, None, span)).collect())
            },
        )
    }

    fn TypeVarSpec(spec: ParseNode) -> ParseResult<Vec<TypeVarDecl>> {
        match_nodes!(spec.into_children();
            [IdList(ids)..] => {
                Ok(ids.flatten().map(|(identifier, span)| TypeVarDecl::new(identifier, span)).collect())
            }
        )
    }

    fn ConsSpec(spec: ParseNode) -> ParseResult<Vec<IdDecl<ConstructorId>>> {
        match_nodes!(spec.into_children();
            [IdsDecl(decls)..] => {
                Ok(decls.flatten().map(IdDecl::retag).collect())
            }
        )
    }

    fn Init(init: ParseNode) -> ParseResult<ProcessExpr> {
        match_nodes!(init.into_children();
            [ProcExpr(expr)] => {
                Ok(expr)
            }
        )
    }

    fn ProcSpec(spec: ParseNode) -> ParseResult<Vec<ProcDecl>> {
        match_nodes!(spec.into_children();
            [ProcDecl(decls)..] => {
                Ok(decls.collect())
            },
        )
    }

    fn ProcDecl(decl: ParseNode) -> ParseResult<ProcDecl> {
        let span = decl.as_span();
        match_nodes!(decl.into_children();
            [Id(identifier), VarsDeclList(params), ProcExpr(body)] => {
                Ok(ProcDecl {
                    identifier,
                    params,
                    body,
                    span: span.into(),
                })
            },
            [Id(identifier), ProcExpr(body)] => {
                Ok(ProcDecl {
                    identifier,
                    params: Vec::new(),
                    body,
                    span: span.into(),
                })
            }
        )
    }

    fn VarSpec(vars: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(vars.into_children();
            [VarsDeclList(ids)..] => {
                Ok(ids.flatten().collect())
            },
        )
    }

    fn IdInfix(identifier: ParseNode) -> ParseResult<String> {
        Ok(identifier.as_str().to_string())
    }

    fn IdInfixList(identifiers: ParseNode) -> ParseResult<Vec<(String, Span)>> {
        Ok(identifiers
            .into_children()
            .map(|node| (node.as_str().to_string(), node.as_span().into()))
            .collect())
    }

    fn IdsDecl(decl: ParseNode) -> ParseResult<Vec<IdDecl>> {
        match_nodes!(decl.into_children();
            [IdInfixList(identifiers), SortExpr(sort)] => {
                let id_decls = identifiers.into_iter().map(|(identifier, span)| {
                    IdDecl::new(identifier, sort.clone(), span)
                }).collect();

                Ok(id_decls)
            },
        )
    }

    fn EqnSpec(spec: ParseNode) -> ParseResult<Vec<EqnSpec>> {
        let span = spec.as_span();
        let mut ids = Vec::new();

        match_nodes!(spec.into_children();
            [VarSpec(variables), EqnDecl(decls)..] => {
                ids.push(EqnSpecData {
                    variables,
                    equations: decls.collect(),
                    id: None,
                }.spanned(span.into()));
            },
            [EqnDecl(decls)..] => {
                ids.push(EqnSpecData { variables: Vec::new(), equations: decls.collect(), id: None }.spanned(span.into()));
            },
        );

        Ok(ids)
    }

    fn EqnDecl(decl: ParseNode) -> ParseResult<EqnDecl> {
        let span = decl.as_span();
        match_nodes!(decl.into_children();
            [DataExpr(condition), DataExpr(lhs), DataExpr(rhs)] => {
                Ok(EqnDecl { condition: Some(condition), lhs, rhs, span: span.into(), id: None })
            },
            [DataExpr(lhs), DataExpr(rhs)] => {
                Ok(EqnDecl { condition: None, lhs, rhs, span: span.into(), id: None })
            },
        )
    }

    fn ActionRenameRuleSpec(spec: ParseNode) -> ParseResult<Vec<ActionRenameDecl>> {
        match_nodes!(spec.into_children();
            [VarSpec(variables_specification), ActionRenameRule(renames)..] => {
                Ok(renames.map(|rename_rule| {
                    ActionRenameDecl { variables_specification: variables_specification.clone(), rename_rule }
                }).collect())
            },
            [ActionRenameRule(renames)..] => {
                Ok(renames.map(|rename_rule| {
                    ActionRenameDecl { variables_specification: Vec::new(), rename_rule }
                }).collect())
            },
        )
    }

    fn ActionRenameRule(input: ParseNode) -> ParseResult<ActionRenameRule> {
        match_nodes!(input.into_children();
            [DataExpr(condition), Action(action), ActionRenameRuleRHS(rhs)] => {
                Ok(ActionRenameRule { condition: Some(condition), action, rhs })
            },
            [Action(action), ActionRenameRuleRHS(rhs)] => {
                Ok(ActionRenameRule { condition: None, action, rhs })
            },
        )
    }

    fn ActionRenameRuleRHS(input: ParseNode) -> ParseResult<ActionRHS> {
        match_nodes!(input.into_children();
            [Action(action)] => {
                Ok(ActionRHS::Action(action))
            },
            [MultiActTau(_)] => {
                Ok(ActionRHS::Tau)
            },
            [ProcExprDelta(_)] => {
                Ok(ActionRHS::Delta)
            },
        )
    }

    fn FormSpec(input: ParseNode) -> ParseResult<StateFrm> {
        match_nodes!(input.into_children();
            [StateFrm(formula)] => {
                Ok(formula)
            },
        )
    }

    pub(crate) fn StateFrmSpec(spec: ParseNode) -> ParseResult<UntypedStateFrmSpec> {
        let span = spec.as_span();
        let decls = collect_state_frm_spec(spec)?;
        Ok(UntypedStateFrmSpec {
            data_specification: decls.data.into_data_specification(),
            action_declarations: decls.action_declarations,
            formula: decls.formula.ok_or(Error::new_from_span(
                ErrorVariant::CustomError {
                    message: "No state formula found in the state formula specification".to_string(),
                },
                span,
            ))?,
        })
    }

    fn EOI(_input: ParseNode) -> ParseResult<()> {
        Ok(())
    }
}
