use indoc::indoc;
use merc_syntax::UntypedDataSpecification;
use pest::Parser;

use merc_syntax::Mcrl2Parser;
use merc_syntax::Rule;
use merc_syntax::SortExpressionKind;
use merc_syntax::UntypedProcessSpecification;
use merc_syntax::UntypedStateFrmSpec;
use merc_syntax::parse_sortexpr;
use merc_utilities::test_logger;
use test_case::test_case;

/// `DataExprIn`, `DataExprIntDiv`, and `DataExprMod` used to be plain string
/// matches without a negative lookahead (`!Id`). This meant that identifiers
/// whose names *start* with `in`, `div`, or `mod` were incorrectly tokenised
/// as a keyword followed by the rest of the identifier.
#[test]
fn keywords_are_not_prefix_of_identifiers() {
    // `index` starts with `in` — must parse as a plain identifier.
    UntypedProcessSpecification::parse("map index: Bool -> Bool;")
        .expect("`index` must not be split into keyword `in` + `dex`");
    // `divides` starts with `div`.
    UntypedProcessSpecification::parse("map divides: Bool -> Bool;")
        .expect("`divides` must not be split into keyword `div` + `ides`");
    // `modern` starts with `mod`.
    UntypedProcessSpecification::parse("map modern: Bool -> Bool;")
        .expect("`modern` must not be split into keyword `mod` + `ern`");
}

/// `Number` used to be a non-atomic rule (`{ ASCII_DIGIT+ }`), which in pest
/// causes implicit whitespace to be absorbed *after* the digits. That meant
/// tokens like `"3 "` could match `Number`, and boundary tests such as
/// `"3 div 2"` could mis-parse because the parser consumed the trailing space
/// as part of the number and left `div` unrecognised as an operator.
#[test]
fn number_token_does_not_absorb_whitespace() {
    // A Number followed immediately by a keyword must still parse correctly.
    // Parsing fails on the leading `val(...)` wrapper in a data-specification
    // context, so use a minimal expression that forces a Number-then-keyword
    // boundary: `3 div 2`.
    let parsed = Mcrl2Parser::parse(Rule::Number, "3 extra");
    // The Number rule must match exactly the digit run, not consume the space.
    assert!(parsed.is_ok());
    assert_eq!(
        parsed.unwrap().as_str(),
        "3",
        "Number must not consume trailing whitespace"
    );
}

#[test]
fn test_parse_ifthen() {
    let expr = "init a -> b <> b;";

    match UntypedProcessSpecification::parse(expr) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            panic!("Failed to parse expression: {}", e);
        }
    }
}

#[test]
fn test_parse_keywords() {
    let expr = "map or : Boolean # Boolean -> Boolean ;";

    match UntypedProcessSpecification::parse(expr) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            panic!("Failed to parse expression: {}", e);
        }
    }
}

#[test]
fn test_parse_sort_spec() {
    let sort_spec = indoc! {"
        sort D = Bool -> Int -> Bool;
        

        % Test
        F     = struct d1 | d2;
        Error = struct e;
    "};

    match UntypedProcessSpecification::parse(sort_spec) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            panic!("Failed to parse expression: {}", e);
        }
    }
}

/// A `type_var` block declares names that are bound for the rest of the specification: every
/// later occurrence of one of them in sort-expression position must parse as a
/// [SortExpressionKind::TypeVar], not a [SortExpressionKind::Reference] — in a constructor's
/// sort, a map's sort, and a `var`-block binder sort alike.
#[test]
fn test_parse_type_var_spec() {
    let spec = indoc! {"
        type_var S;

        cons []: List(S);
             |>: S # List(S) -> List(S);

        map in: S # List(S) -> Bool;

        var d: S;
            s: List(S);
        eqn in(d, []) = false;
    "};

    let parsed = UntypedProcessSpecification::parse(spec).expect("the type_var spec should parse");
    let data = &parsed.data_specification;

    assert_eq!(data.type_var_declarations.len(), 1);
    assert_eq!(data.type_var_declarations[0].identifier, "S");

    // `|>: S # List(S) -> List(S)`: `S` must become `TypeVar` both bare and inside `List(...)`.
    let cons_sort = &data.constructor_declarations[1].sort;
    let SortExpressionKind::Function { domain, range } = &cons_sort.node else {
        panic!("expected a function sort, got {:?}", cons_sort.node);
    };
    let SortExpressionKind::Product { lhs, rhs } = &domain.node else {
        panic!("expected a product domain, got {:?}", domain.node);
    };
    assert!(matches!(&lhs.node, SortExpressionKind::TypeVar(name) if name == "S"));
    assert!(is_list_of_type_var(rhs, "S"));
    assert!(is_list_of_type_var(range, "S"));

    // The `var d: S;` binder sort is rewritten too, not just declaration-level sorts.
    assert!(matches!(
        &data.equation_declarations[0].variables[0].sort.node,
        SortExpressionKind::TypeVar(name) if name == "S"
    ));
}

/// Whether `sort` is `List(TypeVar(name))`.
fn is_list_of_type_var(sort: &merc_syntax::SortExpression, name: &str) -> bool {
    match &sort.node {
        SortExpressionKind::Complex(_, inner) => {
            matches!(&inner.node, SortExpressionKind::TypeVar(inner_name) if inner_name == name)
        }
        _ => false,
    }
}

#[test]
fn test_parse_regular_expression() {
    let spec = "[true++false]true";

    match UntypedStateFrmSpec::parse(spec) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            panic!("Failed to parse expression: {}", e);
        }
    }
}

#[test]
fn test_parse_procexpr() {
    test_logger();

    use indoc::indoc;

    let spec: &str = indoc! {"init
        true -> delta <> delta;
    "};

    match UntypedProcessSpecification::parse(spec) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            panic!("Failed to parse expression: {}", e);
        }
    }
}

#[test]
fn test_parse_statefrm() {
    test_logger();

    use indoc::indoc;

    let spec: &str = indoc! {"<b> <a> exists b: Bool . b && !b"};

    match UntypedStateFrmSpec::parse(spec) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            panic!("Failed to parse expression: {}", e);
        }
    }
}

#[test]
fn test_sort_precedence() {
    let term = "Bool # Int -> Int -> Bool";

    match Mcrl2Parser::parse(Rule::SortExpr, term) {
        Ok(result) => {
            print!("{}", parse_sortexpr(result).unwrap());
        }
        Err(e) => {
            panic!("{}", e);
        }
    }
}

/// Parses every base data specification shipped in `spec/`.
#[test_case(include_str!("../spec/bool.mcrl2") ; "bool.mcrl2")]
#[test_case(include_str!("../spec/int.mcrl2") ; "int.mcrl2")]
#[test_case(include_str!("../spec/nat.mcrl2") ; "nat.mcrl2")]
#[test_case(include_str!("../spec/pos.mcrl2") ; "pos.mcrl2")]
#[test_case(include_str!("../spec/real.mcrl2") ; "real.mcrl2")]
#[test_case(include_str!("../spec/list.mcrl2") ; "list.mcrl2")]
#[test_case(include_str!("../spec/set.mcrl2") ; "set.mcrl2")]
#[test_case(include_str!("../spec/fset.mcrl2") ; "fset.mcrl2")]
#[test_case(include_str!("../spec/bag.mcrl2") ; "bag.mcrl2")]
#[test_case(include_str!("../spec/fbag.mcrl2") ; "fbag.mcrl2")]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_data_spec(spec: &str) {
    match UntypedDataSpecification::parse(spec) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            panic!("Failed to parse expression: {}", e);
        }
    }
}

/// Parses every machine-number (`*64`) specification, ported from the mCRL2
/// code-generation `.spec` files.
#[test_case(include_str!("../spec/machine_word.mcrl2") ; "machine_word.mcrl2")]
#[test_case(include_str!("../spec/pos64.mcrl2") ; "pos64.mcrl2")]
#[test_case(include_str!("../spec/nat64.mcrl2") ; "nat64.mcrl2")]
#[test_case(include_str!("../spec/int64.mcrl2") ; "int64.mcrl2")]
#[test_case(include_str!("../spec/real64.mcrl2") ; "real64.mcrl2")]
#[test_case(include_str!("../spec/list64.mcrl2") ; "list64.mcrl2")]
#[test_case(include_str!("../spec/set64.mcrl2") ; "set64.mcrl2")]
#[test_case(include_str!("../spec/fset64.mcrl2") ; "fset64.mcrl2")]
#[test_case(include_str!("../spec/bag64.mcrl2") ; "bag64.mcrl2")]
#[test_case(include_str!("../spec/fbag64.mcrl2") ; "fbag64.mcrl2")]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_machine_number_spec(spec: &str) {
    match UntypedDataSpecification::parse(spec) {
        Ok(result) => {
            println!("{}", result);
        }
        Err(e) => {
            panic!("Failed to parse: {}", e);
        }
    }
}

/// `EqnSpec`'s span covers the whole `var ... eqn ...` block.
#[test]
fn eqn_spec_span_covers_the_whole_block() {
    let text = "sort D;\nvar x: D;\neqn x = x;";
    let spec = UntypedDataSpecification::parse(text).expect("the specification should parse");
    let eqn_spec = &spec.equation_declarations[0];

    let block = &text[eqn_spec.span.start..eqn_spec.span.end];
    assert_eq!(
        block, "var x: D;\neqn x = x;",
        "span should cover both the `var` and `eqn` sections"
    );
}

/// A block with no `var` section starts its span at `eqn`, not at some earlier declaration.
#[test]
fn eqn_spec_span_without_a_var_section_starts_at_eqn() {
    let text = "map f: Bool;\neqn f = true;";
    let spec = UntypedDataSpecification::parse(text).expect("the specification should parse");
    let eqn_spec = &spec.equation_declarations[0];

    let block = &text[eqn_spec.span.start..eqn_spec.span.end];
    assert_eq!(block, "eqn f = true;");
}

/// A grouped declaration (`sort A, B, C;`, and its `cons`/`map`/`var`/`glob`/`act` siblings) gives
/// each identifier its own precise span.
#[test]
fn grouped_sort_declarations_get_distinct_precise_spans() {
    let text = "sort A, B, C;";
    let spec = UntypedDataSpecification::parse(text).expect("the specification should parse");
    let spans: Vec<&str> = spec
        .sort_declarations
        .iter()
        .map(|decl| &text[decl.span.start..decl.span.end])
        .collect();
    assert_eq!(spans, ["A", "B", "C"]);
}

/// The `sort A = Bool;` alias form (never grouped — it only ever names one sort) gets a span
/// precisely covering the identifier, not the whole `A = Bool;`.
#[test]
fn sort_alias_declaration_span_is_precisely_the_identifier() {
    let text = "sort L = List(Nat);";
    let spec = UntypedDataSpecification::parse(text).expect("the specification should parse");
    let decl = &spec.sort_declarations[0];
    assert_eq!(&text[decl.span.start..decl.span.end], "L");
}

/// A `struct` sort expression's own constructor/projection/recogniser names each get a precise
/// span of their own.
#[test]
fn struct_constructor_projection_and_recogniser_get_precise_spans() {
    let text = "sort D = struct c1(a1: Bool, Nat)?is_c1 | c2;";
    let spec = UntypedDataSpecification::parse(text).expect("the specification should parse");
    let merc_syntax::SortExpressionKind::Struct { inner } = &spec.sort_declarations[0].expr.as_ref().unwrap().node
    else {
        panic!("expected a structured sort");
    };

    let c1 = &inner[0];
    assert_eq!(&text[c1.name.span.start..c1.name.span.end], "c1");
    let (a1, _) = &c1.args[0];
    let a1 = a1.as_ref().expect("the first argument names its projection");
    assert_eq!(&text[a1.span.start..a1.span.end], "a1");
    let (anonymous, _) = &c1.args[1];
    assert!(anonymous.is_none(), "the second argument has no projection name");
    let is_c1 = c1.projection.as_ref().expect("c1 declares a recogniser");
    assert_eq!(&text[is_c1.span.start..is_c1.span.end], "is_c1");

    let c2 = &inner[1];
    assert_eq!(&text[c2.name.span.start..c2.name.span.end], "c2");
    assert!(c2.projection.is_none());
}

#[test]
fn grouped_constructor_declarations_get_distinct_precise_spans() {
    let text = "sort D; cons c1, c2: D;";
    let spec = UntypedDataSpecification::parse(text).expect("the specification should parse");
    let spans: Vec<&str> = spec
        .constructor_declarations
        .iter()
        .map(|decl| &text[decl.identifier.span.start..decl.identifier.span.end])
        .collect();
    assert_eq!(spans, ["c1", "c2"]);
}

/// Each `ActDecl` exploded out of a grouped `act a, b: Bool;` gets its own precise identifier
/// span.
#[test]
fn grouped_action_declarations_get_distinct_precise_spans() {
    let text = "act a, b: Bool;";
    let spec = UntypedProcessSpecification::parse(text).expect("the specification should parse");
    let identifier_spans: Vec<&str> = spec
        .action_declarations
        .iter()
        .map(|decl| &text[decl.identifier.span.start..decl.identifier.span.end])
        .collect();
    assert_eq!(identifier_spans, ["a", "b"]);
    for decl in &spec.action_declarations {
        assert_eq!(&text[decl.span.start..decl.span.end], "a, b: Bool;");
    }
}

/// `Assignment` (a process instantiation's `x = e`, as in `P(x = 1)`) carries a span precisely
/// covering the identifier.
#[test]
fn assignment_span_is_precisely_the_identifier() {
    let text = "proc P(x: Bool) = delta; init P(x = true);";
    let spec = UntypedProcessSpecification::parse(text).expect("the specification should parse");
    let merc_syntax::ProcessExprKind::Id(_, assignments) = &spec.init.expect("the fixture has an init").node else {
        panic!("expected `init` to be a process instantiation");
    };
    let assignment = &assignments[0];
    assert_eq!(&text[assignment.span.start..assignment.span.end], "x");
}

/// `ProcDecl` carries both its own span and the identifier span.
#[test]
fn process_declaration_identifier_span_is_precisely_the_identifier() {
    let text = "proc P(x: Bool) = delta;\nproc Q = tau;\ninit P(true);";
    let spec = UntypedProcessSpecification::parse(text).expect("the specification should parse");
    let identifier_spans: Vec<&str> = spec
        .process_declarations
        .iter()
        .map(|decl| &text[decl.identifier.span.start..decl.identifier.span.end])
        .collect();
    assert_eq!(identifier_spans, ["P", "Q"]);
    let whole_spans: Vec<&str> = spec
        .process_declarations
        .iter()
        .map(|decl| &text[decl.span.start..decl.span.end])
        .collect();
    assert_eq!(whole_spans, ["P(x: Bool) = delta;", "Q = tau;"]);
}

/// `PropVarInst` (`X(0)`) carries both its own span, covering `Id(args)` in full, and its
/// `identifier`'s own span, covering only `Id` — mirroring `ActionName`, so a later pass (hover,
/// go-to-definition) can point at just the name rather than the whole instantiation.
#[test]
fn prop_var_inst_identifier_span_is_precisely_the_identifier() {
    let text = "pbes mu X(n: Nat) = val(n == n); init X(0);";
    let pbes = merc_syntax::UntypedPbes::parse(text).expect("the specification should parse");
    assert_eq!(&text[pbes.init.span.start..pbes.init.span.end], "X(0)");
    assert_eq!(
        &text[pbes.init.identifier.span.start..pbes.init.identifier.span.end],
        "X"
    );
}

/// `PropVarDecl` carries both its own span and the identifier span
#[test]
fn prop_var_decl_identifier_span_is_precisely_the_identifier() {
    let text = "pbes mu X(n: Nat) = val(n == n) && Y; nu Y = val(true); init X(0);";
    let pbes = merc_syntax::UntypedPbes::parse(text).expect("the specification should parse");
    let identifier_spans: Vec<&str> = pbes
        .equations
        .iter()
        .map(|eqn| &text[eqn.variable.identifier.span.start..eqn.variable.identifier.span.end])
        .collect();
    assert_eq!(identifier_spans, ["X", "Y"]);
    let whole_spans: Vec<&str> = pbes
        .equations
        .iter()
        .map(|eqn| &text[eqn.variable.span.start..eqn.variable.span.end])
        .collect();
    assert_eq!(whole_spans, ["X(n: Nat)", "Y"]);
}
