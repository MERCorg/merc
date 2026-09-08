use std::convert::Infallible;
use std::fmt::Write;
use std::sync::LazyLock;

use indoc::formatdoc;

use merc_syntax::ComplexSort;
use merc_syntax::ConstructorDecl;
use merc_syntax::SortExpression;
use merc_syntax::SortExpressionKind;
use merc_syntax::SourceMap;
use merc_syntax::Traverse;
use merc_syntax::TypeVarId;
use merc_syntax::UntypedDataSpecification;
use merc_utilities::MercError;

use crate::BASIC_SORT_NAMES;
use crate::NumberEncoding;
use crate::apply_sorts_in_spec;
use crate::resolve_type_var_ids;

/// Parses a bundled `spec/*.mcrl2` file, or an equally self-contained
/// hand-written template string (`BUILTIN_SCHEME_TEMPLATE`), with no
/// `SourceMap` involved: the result's spans are meaningless outside `text`
/// itself. Used for [CONTAINER_TEMPLATES] and `BUILTIN_SCHEME_TEMPLATE`,
/// the two sources `build_polymorphic_schemes` draws from — nothing built
/// this way is ever rendered.
pub(crate) fn parse_template_bare(text: &str) -> UntypedDataSpecification {
    let mut spec = UntypedDataSpecification::parse(text).expect("the bundled templates parse");
    resolve_type_var_ids(&mut spec).expect("the bundled template's type_var block resolves");
    spec
}

/// Registers `text` under `name` as a virtual source in `sources` (see
/// [SourceMap::add_virtual]) and parses it padded to that registration's base
/// offset, so every span pest reports already lands at the correct global
/// offset — the same padding technique [merc_syntax::imports] uses for
/// `%import`.
fn parse_template(sources: &mut SourceMap, name: &str, text: &str) -> UntypedDataSpecification {
    parse_generated(sources, name, text).expect("the bundled templates parse")
}

/// As [parse_template], but for content this module generated itself
/// (`formatdoc!`/`write!` output rather than a bundled `spec/*.mcrl2` file)
/// and so, unlike a bundled template, might not parse — a bug in the
/// generator rather than in a `spec/*.mcrl2` file. Returns the parse error
/// instead of panicking, so a caller can report it.
fn parse_generated(sources: &mut SourceMap, name: &str, text: &str) -> Result<UntypedDataSpecification, MercError> {
    let id = sources.add_virtual(name, text.to_string());
    let base = sources.base_offset(id);
    let padded = " ".repeat(base) + text;
    let mut spec = UntypedDataSpecification::parse(&padded)?;
    // As in `parse_template_bare`: resolves a template's own `type_var` block, if
    // it has one. Content this module generates itself (`multi_argument_function_update`,
    // `structured_sort_equations`) never declares one, so this is a no-op there.
    resolve_type_var_ids(&mut spec)?;
    Ok(spec)
}

/// The merged specifications of the five basic sorts (Appendix B.1–B.7) in the
/// recursive binary encoding, registered into `sources` as virtual documents.
fn basic_sorts_binary(sources: &mut SourceMap) -> UntypedDataSpecification {
    let mut result = UntypedDataSpecification::default();
    result.merge(&parse_template(
        sources,
        "<builtin>/bool.mcrl2",
        include_str!("../../../syntax/spec/bool.mcrl2"),
    ));
    result.merge(&parse_template(
        sources,
        "<builtin>/pos.mcrl2",
        include_str!("../../../syntax/spec/pos.mcrl2"),
    ));
    result.merge(&parse_template(
        sources,
        "<builtin>/int.mcrl2",
        include_str!("../../../syntax/spec/int.mcrl2"),
    ));
    result.merge(&parse_template(
        sources,
        "<builtin>/nat.mcrl2",
        include_str!("../../../syntax/spec/nat.mcrl2"),
    ));
    result.merge(&parse_template(
        sources,
        "<builtin>/real.mcrl2",
        include_str!("../../../syntax/spec/real.mcrl2"),
    ));
    result
}

/// The same five basic sorts in the 64-bit machine-word encoding. `Bool` is
/// shared with the binary encoding; the numeric sorts come from the `*64`
/// templates, which are defined in terms of the `@word` sort that
/// `machine_word.mcrl2` declares.
fn basic_sorts_machine_word(sources: &mut SourceMap) -> UntypedDataSpecification {
    let mut result = UntypedDataSpecification::default();
    result.merge(&parse_template(
        sources,
        "<builtin>/bool.mcrl2",
        include_str!("../../../syntax/spec/bool.mcrl2"),
    ));
    result.merge(&parse_template(
        sources,
        "<builtin>/machine_word.mcrl2",
        include_str!("../../../syntax/spec/machine_word.mcrl2"),
    ));
    result.merge(&parse_template(
        sources,
        "<builtin>/pos64.mcrl2",
        include_str!("../../../syntax/spec/pos64.mcrl2"),
    ));
    result.merge(&parse_template(
        sources,
        "<builtin>/int64.mcrl2",
        include_str!("../../../syntax/spec/int64.mcrl2"),
    ));
    result.merge(&parse_template(
        sources,
        "<builtin>/nat64.mcrl2",
        include_str!("../../../syntax/spec/nat64.mcrl2"),
    ));
    result.merge(&parse_template(
        sources,
        "<builtin>/real64.mcrl2",
        include_str!("../../../syntax/spec/real64.mcrl2"),
    ));
    result
}

/// The raw, uninstantiated container and function-update templates, parsed
/// once.
pub(crate) struct ContainerTemplates {
    list: UntypedDataSpecification,
    set: UntypedDataSpecification,
    fset: UntypedDataSpecification,
    bag: UntypedDataSpecification,
    fbag: UntypedDataSpecification,
    function_update: UntypedDataSpecification,
}

impl ContainerTemplates {
    /// All templates, for building the polymorphic signature.
    pub(crate) fn all(&self) -> [&UntypedDataSpecification; 6] {
        [
            &self.list,
            &self.set,
            &self.fset,
            &self.bag,
            &self.fbag,
            &self.function_update,
        ]
    }
}

/// The container templates in the recursive binary encoding, only used for the
/// signatures.
///
/// This is also the set the polymorphic signature is built from: the `*64`
/// templates declare exactly the same operations with the same sorts (they
/// differ only in their defining equations), so the *signature* of the
/// container operations does not depend on the number encoding.
pub(crate) static CONTAINER_TEMPLATES: LazyLock<ContainerTemplates> = LazyLock::new(|| ContainerTemplates {
    list: parse_template_bare(include_str!("../../../syntax/spec/list.mcrl2")),
    set: parse_template_bare(include_str!("../../../syntax/spec/set.mcrl2")),
    fset: parse_template_bare(include_str!("../../../syntax/spec/fset.mcrl2")),
    bag: parse_template_bare(include_str!("../../../syntax/spec/bag.mcrl2")),
    fbag: parse_template_bare(include_str!("../../../syntax/spec/fbag.mcrl2")),
    function_update: parse_template_bare(include_str!("../../../syntax/spec/function_update.mcrl2")),
});

/// The container templates in the recursive binary encoding, registered into
/// `sources` as virtual documents — the content-producing counterpart of
/// [CONTAINER_TEMPLATES], used wherever the result joins a [DataSpecification]'s
/// `system` and so needs spans that render correctly.
///
/// [DataSpecification]: crate::DataSpecification
fn container_templates_binary(sources: &mut SourceMap) -> ContainerTemplates {
    ContainerTemplates {
        list: parse_template(
            sources,
            "<builtin>/list.mcrl2",
            include_str!("../../../syntax/spec/list.mcrl2"),
        ),
        set: parse_template(
            sources,
            "<builtin>/set.mcrl2",
            include_str!("../../../syntax/spec/set.mcrl2"),
        ),
        fset: parse_template(
            sources,
            "<builtin>/fset.mcrl2",
            include_str!("../../../syntax/spec/fset.mcrl2"),
        ),
        bag: parse_template(
            sources,
            "<builtin>/bag.mcrl2",
            include_str!("../../../syntax/spec/bag.mcrl2"),
        ),
        fbag: parse_template(
            sources,
            "<builtin>/fbag.mcrl2",
            include_str!("../../../syntax/spec/fbag.mcrl2"),
        ),
        function_update: parse_template(
            sources,
            "<builtin>/function_update.mcrl2",
            include_str!("../../../syntax/spec/function_update.mcrl2"),
        ),
    }
}

/// The container templates whose equations are expressed in terms of the
/// machine-word numeric sorts. `function_update.mcrl2` mentions no numbers, so
/// it is shared with the binary encoding.
fn container_templates_machine_word(sources: &mut SourceMap) -> ContainerTemplates {
    ContainerTemplates {
        list: parse_template(
            sources,
            "<builtin>/list64.mcrl2",
            include_str!("../../../syntax/spec/list64.mcrl2"),
        ),
        set: parse_template(
            sources,
            "<builtin>/set64.mcrl2",
            include_str!("../../../syntax/spec/set64.mcrl2"),
        ),
        fset: parse_template(
            sources,
            "<builtin>/fset64.mcrl2",
            include_str!("../../../syntax/spec/fset64.mcrl2"),
        ),
        bag: parse_template(
            sources,
            "<builtin>/bag64.mcrl2",
            include_str!("../../../syntax/spec/bag64.mcrl2"),
        ),
        fbag: parse_template(
            sources,
            "<builtin>/fbag64.mcrl2",
            include_str!("../../../syntax/spec/fbag64.mcrl2"),
        ),
        function_update: parse_template(
            sources,
            "<builtin>/function_update.mcrl2",
            include_str!("../../../syntax/spec/function_update.mcrl2"),
        ),
    }
}

/// The container templates to instantiate for `encoding`, registered into
/// `sources` so their spans render correctly.
fn container_templates(sources: &mut SourceMap, encoding: NumberEncoding) -> ContainerTemplates {
    match encoding {
        NumberEncoding::Binary => container_templates_binary(sources),
        NumberEncoding::MachineWord => container_templates_machine_word(sources),
    }
}

/// The Appendix-B equations of the built-in operator *schemes* at `sort`: the
/// conditional `if`, and the reflexive/derived cases of the comparison
/// operators.
///
/// Only equations are emitted; the `map` signatures are omitted deliberately.
/// The comparison operators and `if` exist for *every* sort, so inference types
/// them as polymorphic schemes instantiated per occurrence (their signatures
/// live in `crate::BUILTIN_SCHEME_TEMPLATE`, resolved through
/// `POLYMORPHIC_SIGNATURE` like the container operations) rather than declaring
/// one overload per sort.
pub(crate) fn builtin_operator_equations(sources: &mut SourceMap, sort: &str) -> UntypedDataSpecification {
    // The variable names are qualified by sort so that merging the blocks of
    // several sorts cannot collide, here or with a user declaration.
    let text = formatdoc! {"
        var x_{sort}, y_{sort}: {sort};
        eqn x_{sort} == x_{sort} = true;
            x_{sort} != y_{sort} = !(x_{sort} == y_{sort});
            x_{sort} < x_{sort} = false;
            x_{sort} <= x_{sort} = true;
            x_{sort} > y_{sort} = y_{sort} < x_{sort};
            x_{sort} >= y_{sort} = y_{sort} <= x_{sort};
            if(true, x_{sort}, y_{sort}) = x_{sort};
            if(false, x_{sort}, y_{sort}) = y_{sort};
    "};
    parse_template(sources, &format!("<builtin>/schemes/{sort}.mcrl2"), &text)
}

/// Returns a standard data specification containing the standard sorts and their
/// associated constructors, mappings, and equations, in the given `encoding`,
/// registered into `sources`.
pub(crate) fn basic_sort_data_specification(
    sources: &mut SourceMap,
    encoding: NumberEncoding,
) -> UntypedDataSpecification {
    let mut result = match encoding {
        NumberEncoding::Binary => basic_sorts_binary(sources),
        NumberEncoding::MachineWord => basic_sorts_machine_word(sources),
    };

    for sort in BASIC_SORT_NAMES {
        result.merge(&builtin_operator_equations(sources, sort));
    }
    result
}

/// Constructs a data specification for a standard sort, in the given
/// `encoding`, registered into `sources`.
pub(crate) fn standard_sort(
    sources: &mut SourceMap,
    sort: &SortExpression,
    encoding: NumberEncoding,
) -> UntypedDataSpecification {
    let templates = container_templates(sources, encoding);

    if let SortExpressionKind::Complex(complex, sort) = &sort.node {
        let template = match complex {
            ComplexSort::List => &templates.list,
            ComplexSort::Set => &templates.set,
            ComplexSort::FSet => &templates.fset,
            ComplexSort::Bag => &templates.bag,
            ComplexSort::FBag => &templates.fbag,
        };

        replace_sort(template, "S", sort)
    } else if let SortExpressionKind::Function { domain, range } = &sort.node {
        // In the specification we define the function S -> T.
        let spec = replace_sort(&templates.function_update, "S", domain);
        replace_sort(&spec, "T", range)
    } else if let SortExpressionKind::FlattenedFunction { domain, range } = &sort.node {
        // A multi-argument function sort: the bundled template's single index
        // variable `S` cannot stand for a product, so its equations are built
        // directly instead of substituted into the template.
        multi_argument_function_update(sources, domain, range)
    } else {
        unreachable!("The given sort {} is not a standard sort", sort);
    }
}

/// Generates the function-update operators (`@func_update`,
/// `@func_update_stable`, `@is_not_an_update`, `@if_always_else`, Appendix
/// B.11 / `function_update.mcrl2`) for a function sort of arity
/// `domain.len() > 1`, generalizing the bundled single-argument template to
/// the flattened domain `D_0 # ... # D_{n-1} -> T`.
///
/// The single index variable `x`/`y` of the unary template becomes a tuple
/// `x0, ..., x{n-1}`: two tuples are compared componentwise for equality
/// (`&&` of `==`) and ordered lexicographically (`<`) wherever the template
/// orders or compares a single index — `<` canonicalizes the order nested
/// `@func_update_stable` chains normalize to, so rewriting stays confluent
/// regardless of the syntactic nesting order of `f[a -> b][c -> d]`-style
/// updates. This mirrors [structured_sort_equations]'s `lexicographic` helper,
/// which solves the same problem for a constructor's argument tuple.
pub(crate) fn multi_argument_function_update(
    sources: &mut SourceMap,
    domain: &[SortExpression],
    range: &SortExpression,
) -> UntypedDataSpecification {
    debug_assert!(
        domain.len() > 1,
        "single-argument function updates are generated from the bundled template"
    );

    let function_sort: SortExpression = SortExpressionKind::FlattenedFunction {
        domain: domain.to_vec(),
        range: Box::new(range.clone()),
    }
    .into();
    let domain_sorts = domain
        .iter()
        .map(SortExpression::to_string)
        .collect::<Vec<_>>()
        .join(" # ");

    let xs: Vec<String> = (0..domain.len()).map(|i| format!("x{i}")).collect();
    let ys: Vec<String> = (0..domain.len()).map(|i| format!("y{i}")).collect();
    let x_args = xs.join(", ");
    let y_args = ys.join(", ");

    let equal = |a: &[String], b: &[String]| -> String {
        a.iter()
            .zip(b)
            .map(|(l, r)| format!("{l} == {r}"))
            .collect::<Vec<_>>()
            .join(" && ")
    };
    // Lexicographic order over the index tuple, exactly as `structured_sort_equations`'s
    // `lexicographic` closure builds it for constructor arguments.
    let less = |a: &[String], b: &[String]| -> String {
        let last = a.len() - 1;
        let mut expr = format!("{} < {}", a[last], b[last]);
        for i in (0..last).rev() {
            expr = format!("{} < {} || ({} == {} && ({expr}))", a[i], b[i], a[i], b[i]);
        }
        expr
    };

    let x_eq_y = equal(&xs, &ys);
    let x_neq_y = format!("!({x_eq_y})");
    let y_lt_x = less(&ys, &xs);
    let x_lt_y = less(&xs, &ys);

    let mut spec = String::new();
    writeln!(
        spec,
        "map @func_update: {function_sort} # {domain_sorts} # {range} -> {function_sort};"
    )
    .unwrap();
    writeln!(
        spec,
        "    @func_update_stable: {function_sort} # {domain_sorts} # {range} -> {function_sort};"
    )
    .unwrap();
    writeln!(spec, "    @is_not_an_update: {function_sort} -> Bool;").unwrap();
    writeln!(
        spec,
        "    @if_always_else: Bool # {function_sort} # {function_sort} -> {function_sort};"
    )
    .unwrap();

    writeln!(spec, "var").unwrap();
    for (i, argument_sort) in domain.iter().enumerate() {
        writeln!(spec, "    x{i}, y{i}: {argument_sort};").unwrap();
    }
    writeln!(spec, "    v, w: {range};").unwrap();
    writeln!(spec, "    f: {domain_sorts} -> {range};").unwrap();

    writeln!(
        spec,
        "eqn @is_not_an_update(f) -> @func_update(f,{x_args},v) = \
         @if_always_else(f({x_args}) == v,f,@func_update_stable(f,{x_args},v));"
    )
    .unwrap();
    writeln!(
        spec,
        "    @func_update(@func_update_stable(f,{x_args},w),{x_args},v) = \
         @if_always_else(f({x_args}) == v,f,@func_update_stable(f,{x_args},v));"
    )
    .unwrap();
    writeln!(
        spec,
        "    {y_lt_x} -> @func_update(@func_update_stable(f,{y_args},w), {x_args},v) = \
         @func_update_stable(@func_update(f,{x_args},v),{y_args},w);"
    )
    .unwrap();
    writeln!(
        spec,
        "    {x_lt_y} -> @func_update(@func_update_stable(f,{y_args},w), {x_args},v) = \
         @if_always_else(f({x_args}) == v, \
         @func_update_stable(f,{y_args},w), \
         @func_update_stable(@func_update_stable(f,{y_args},w), {x_args},v));"
    )
    .unwrap();
    writeln!(
        spec,
        "    {x_neq_y} -> @func_update_stable(f,{x_args},v)({y_args}) = f({y_args});"
    )
    .unwrap();
    writeln!(spec, "    @func_update_stable(f,{x_args},v)({x_args}) = v;").unwrap();
    writeln!(
        spec,
        "    @func_update(f,{x_args},v)({y_args}) = if({x_eq_y},v,f({y_args}));"
    )
    .unwrap();

    parse_generated(
        sources,
        &format!("<generated>/function_update({domain_sorts} -> {range}).mcrl2"),
        &spec,
    )
    .unwrap_or_else(|err| {
        panic!(
            "the generated multi-argument function update for '{domain_sorts} -> {range}' does not parse: {err}\n{spec}"
        )
    })
}

/// Replaces the given `type_var`-declared identifier by the given sort
/// expression in the given data specification.
///
/// # Details
///
/// This function can be used to instantiate polymorphic types, for example,
/// replacing `spec`'s bound type variable `S` (declared by `spec`'s own
/// `type_var S;` block) by `Nat` to get a specification for `List(Nat)` out
/// of the `List(S)` template. The substitution covers every sort in the
/// specification, including the binder sorts inside equations (`forall c:S.`
/// in the set/bag templates).
///
/// `identifier` is looked up in `spec.type_var_declarations` to find the
/// [TypeVarId] name resolution already assigned it.
fn replace_sort(spec: &UntypedDataSpecification, identifier: &str, sort: &SortExpression) -> UntypedDataSpecification {
    let mut result = spec.clone();

    let type_var_id = spec
        .type_var_declarations
        .iter()
        .find(|decl| decl.identifier == identifier)
        .and_then(|decl| decl.id)
        .unwrap_or_else(|| panic!("template has no resolved `type_var {identifier}` declaration"));

    apply_sorts_in_spec(&mut result, |expr| -> Result<_, Infallible> {
        Ok(replace_type_var(expr, type_var_id, sort))
    })
    .expect("substitution never fails");

    // `identifier` is now fully substituted away; drop its declaration so that.
    result
        .type_var_declarations
        .retain(|decl| decl.identifier != identifier);

    result
}

/// Replaces every [ResolvedTypeVar] node naming `type_var_id` in `sort` by
/// `result_sort`. See [replace_sort].
fn replace_type_var(sort: &SortExpression, type_var_id: TypeVarId, result_sort: &SortExpression) -> SortExpression {
    sort.clone()
        .apply(|expr| -> Result<Option<SortExpression>, Infallible> {
            if let SortExpressionKind::ResolvedTypeVar(id) = &expr.node
                && *id == type_var_id
            {
                return Ok(Some(result_sort.clone()));
            }

            Ok(None)
        })
        .unwrap()
}

/// Generates the defining equations of a structured sort, following Appendix `B.10`.
///
/// # Details
///
/// Given the constructors `c_1, ..., c_n` of a structured sort, where every
/// constructor `c_i` has arguments of sorts `A_{i,1}, ..., A_{i,k_i}`, this
/// generates the equations defining the recognisers, the projections, and the
/// comparison operators `==`, `<` and `<=` over the constructors.
///
/// Only equations are generated; the abstract sort and the constructor,
/// recogniser and projection declarations are introduced by
/// `desugar_structured_sorts`, which also yields the `constructors` passed
/// here. The result joins the system-defined specification, like the other
/// Appendix-B content.
pub(crate) fn structured_sort_equations(
    sources: &mut SourceMap,
    constructors: &[ConstructorDecl],
) -> Result<UntypedDataSpecification, MercError> {
    // Builds the term `c_i(<prefix>i_0, ..., <prefix>i_{k_i - 1})`, using the
    // bare constructor name when `c_i` takes no arguments.
    let application = |i: usize, prefix: &str| -> String {
        let constructor = &constructors[i];
        if constructor.args.is_empty() {
            constructor.name.node.clone()
        } else {
            let arguments = (0..constructor.args.len())
                .map(|j| format!("{prefix}{i}_{j}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}({arguments})", constructor.name.node)
        }
    };

    // Builds the right-hand side of the `<` or `<=` equation between two equal
    // constructors, i.e. the lexicographic comparison of their arguments where
    // the final argument is compared using `last_op` (`<` or `<=`):
    //   x0 < y0 || (x0 == y0 && (... || (x_{k-2} == y_{k-2} && (x_{k-1} OP y_{k-1}))...))
    let lexicographic = |i: usize, arity: usize, last_op: &str| -> String {
        let mut expr = format!("x{i}_{last} {last_op} y{i}_{last}", last = arity - 1);
        for j in (0..arity - 1).rev() {
            expr = format!("x{i}_{j} < y{i}_{j} || (x{i}_{j} == y{i}_{j} && ({expr}))");
        }
        expr
    };

    let mut spec = String::new();

    // var: one x/y pair per constructor argument.
    let mut vars = String::new();
    for (i, constructor) in constructors.iter().enumerate() {
        for (j, (_, sort)) in constructor.args.iter().enumerate() {
            writeln!(vars, "    x{i}_{j}, y{i}_{j}: {sort};").unwrap();
        }
    }

    // eqn: recogniser, projection and comparison equations.
    let mut eqns = String::new();

    // Recognisers: isC_i(c_i(..)) = true; isC_i(c_j(..)) = false for j != i.
    for (i, constructor) in constructors.iter().enumerate() {
        if let Some(recogniser) = &constructor.projection {
            let recogniser = &recogniser.node;
            writeln!(eqns, "    {recogniser}({}) = true;", application(i, "x")).unwrap();
            for j in 0..constructors.len() {
                if j != i {
                    writeln!(eqns, "    {recogniser}({}) = false;", application(j, "x")).unwrap();
                }
            }
        }
    }

    // Projections: pr_{i,j}(c_i(..)) = x_{i,j}.
    for (i, constructor) in constructors.iter().enumerate() {
        for (j, (projection, _)) in constructor.args.iter().enumerate() {
            if let Some(projection) = projection {
                let projection = &projection.node;
                writeln!(eqns, "    {projection}({}) = x{i}_{j};", application(i, "x")).unwrap();
            }
        }
    }

    // Equality: componentwise on equal constructors, false on distinct ones.
    for (i, constructor) in constructors.iter().enumerate() {
        let equal = if constructor.args.is_empty() {
            "true".to_string()
        } else {
            (0..constructor.args.len())
                .map(|j| format!("x{i}_{j} == y{i}_{j}"))
                .collect::<Vec<_>>()
                .join(" && ")
        };
        writeln!(
            eqns,
            "    {} == {} = {equal};",
            application(i, "x"),
            application(i, "y")
        )
        .unwrap();
        for j in 0..constructors.len() {
            if j != i {
                writeln!(eqns, "    {} == {} = false;", application(i, "x"), application(j, "y")).unwrap();
            }
        }
    }

    // Less-than: lexicographic on equal constructors, by constructor index otherwise.
    for (i, constructor) in constructors.iter().enumerate() {
        let less = if constructor.args.is_empty() {
            "false".to_string()
        } else {
            lexicographic(i, constructor.args.len(), "<")
        };
        writeln!(eqns, "    {} < {} = {less};", application(i, "x"), application(i, "y")).unwrap();
        for j in 0..constructors.len() {
            if i < j {
                writeln!(eqns, "    {} < {} = true;", application(i, "x"), application(j, "y")).unwrap();
            } else if i > j {
                writeln!(eqns, "    {} < {} = false;", application(i, "x"), application(j, "y")).unwrap();
            }
        }
    }

    // Less-than-or-equal: as `<`, but the last argument is compared with `<=`.
    for (i, constructor) in constructors.iter().enumerate() {
        let less_equal = if constructor.args.is_empty() {
            "true".to_string()
        } else {
            lexicographic(i, constructor.args.len(), "<=")
        };
        writeln!(
            eqns,
            "    {} <= {} = {less_equal};",
            application(i, "x"),
            application(i, "y")
        )
        .unwrap();
        for j in 0..constructors.len() {
            if i < j {
                writeln!(eqns, "    {} <= {} = true;", application(i, "x"), application(j, "y")).unwrap();
            } else if i > j {
                writeln!(eqns, "    {} <= {} = false;", application(i, "x"), application(j, "y")).unwrap();
            }
        }
    }

    if vars.is_empty() {
        write!(spec, "eqn\n{eqns}").unwrap();
    } else {
        write!(spec, "var\n{vars}eqn\n{eqns}").unwrap();
    }

    // Named after the first constructor so a parse-error render reads as "the
    // struct with c1, ...", not an opaque, unnumbered "<generated>".
    let name = constructors
        .first()
        .map(|c| format!("<generated>/struct/{}.mcrl2", c.name.node))
        .unwrap_or_else(|| "<generated>/struct/empty.mcrl2".to_string());
    parse_generated(sources, &name, &spec)
}

#[cfg(test)]
mod tests {
    use std::ops::ControlFlow;

    use merc_syntax::ConstructorDecl;
    use merc_syntax::SortExpressionKind;
    use merc_syntax::SourceMap;
    use merc_syntax::Traverse;

    use super::CONTAINER_TEMPLATES;
    use super::UntypedDataSpecification;
    use super::standard_sort;
    use super::structured_sort_equations;
    use crate::DataSpecification;
    use crate::NumberEncoding;

    /// Whether `spec` mentions a bare, unresolved `Reference` sort anywhere.
    /// Used to assert a template's own `S`/`T` no longer shows up this way.
    fn contains_reference(spec: &UntypedDataSpecification) -> bool {
        let has_reference = |sort: &merc_syntax::SortExpression| {
            sort.visit(|expr| {
                if matches!(expr.node, SortExpressionKind::Reference(_)) {
                    ControlFlow::Break(())
                } else {
                    ControlFlow::Continue(())
                }
            })
            .is_some()
        };
        spec.map_declarations.iter().any(|map| has_reference(&map.sort))
            || spec
                .constructor_declarations
                .iter()
                .any(|cons| has_reference(&cons.sort))
    }

    #[test]
    fn test_container_templates_declare_type_var() {
        // `list.mcrl2`/`bag.mcrl2`/... each declare one `type_var S;`, resolved
        // once by `parse_template_bare` — no bare `Reference("S")` should remain.
        for template in [
            &CONTAINER_TEMPLATES.list,
            &CONTAINER_TEMPLATES.set,
            &CONTAINER_TEMPLATES.fset,
            &CONTAINER_TEMPLATES.bag,
            &CONTAINER_TEMPLATES.fbag,
        ] {
            assert_eq!(template.type_var_declarations.len(), 1, "{template}");
            assert_eq!(template.type_var_declarations[0].identifier, "S");
            assert!(
                template.type_var_declarations[0].id.is_some(),
                "resolve_type_var_ids should have assigned an id"
            );
            assert!(
                !contains_reference(template),
                "no bare `S` reference should remain:\n{template}"
            );
        }
    }

    #[test]
    fn test_function_update_template_declares_two_type_vars() {
        // `function_update.mcrl2` declares `type_var S, T;`.
        let names: Vec<&str> = CONTAINER_TEMPLATES
            .function_update
            .type_var_declarations
            .iter()
            .map(|decl| decl.identifier.as_str())
            .collect();
        assert_eq!(names, ["S", "T"]);
        assert!(!contains_reference(&CONTAINER_TEMPLATES.function_update));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_standard_sort_substitutes_type_var_for_element_sort() {
        // `List(Nat)`'s `[]` constructor should end up with sort `List(Nat)` —
        // the type-var-based substitution must produce the same result the old
        // Reference-based one did — and no `type_var` declaration should be left
        // over in the instantiated copy.
        let mut sources = SourceMap::new();
        let checked = DataSpecification::from_untyped_with(
            UntypedDataSpecification::parse("map f: List(Nat);").unwrap(),
            NumberEncoding::default(),
            &mut sources,
        )
        .unwrap();
        let sort = &checked.data_specification().map_declarations[0].sort;

        let generated = standard_sort(&mut sources, sort, NumberEncoding::Binary);
        assert!(generated.type_var_declarations.is_empty(), "{generated}");

        let nil = generated
            .constructor_declarations
            .iter()
            .find(|cons| cons.identifier.node == "[]")
            .expect("List(Nat) should still declare `[]`");
        assert_eq!(nil.sort.to_string(), "List(Nat)");
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_multi_argument_function_gets_generalized_update_operators() {
        // `from_untyped` flattens `Nat # Bool -> Nat` before generating the
        // system-defined specification, so `standard_sort` sees a
        // `FlattenedFunction` domain of length two and takes the
        // multi-argument branch instead of the bundled single-argument
        // template.
        let mut sources = SourceMap::new();
        let checked = DataSpecification::from_untyped_with(
            UntypedDataSpecification::parse("map f: Nat # Bool -> Nat;").unwrap(),
            NumberEncoding::default(),
            &mut sources,
        )
        .unwrap();
        let sort = &checked.data_specification().map_declarations[0].sort;
        let SortExpressionKind::FlattenedFunction { domain, .. } = &sort.node else {
            panic!("expected a flattened function sort: {sort}");
        };
        assert_eq!(domain.len(), 2);

        let generated = standard_sort(&mut sources, sort, NumberEncoding::Binary);
        assert!(
            generated
                .map_declarations
                .iter()
                .any(|map| map.identifier.node == "@func_update"),
            "the multi-argument function sort should still declare @func_update"
        );

        let equations: Vec<String> = generated
            .equation_declarations
            .iter()
            .flat_map(|eqn_spec| &eqn_spec.equations)
            .map(|eqn| eqn.to_string())
            .collect();

        // Both `f` and `@func_update` are applied with the full two-argument
        // index tuple, not the single index the bundled template uses.
        assert!(
            equations.iter().any(|eqn| eqn.contains("f(x0, x1)")),
            "expected a two-argument application of f: {equations:#?}"
        );
        assert!(
            equations.iter().any(|eqn| eqn.contains("@func_update(f, x0, x1, v)")),
            "expected @func_update applied with both index arguments: {equations:#?}"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_multi_argument_function_update_generalizes_to_higher_arities() {
        // The same construction must not be hard-coded to arity two.
        let mut sources = SourceMap::new();
        let checked = DataSpecification::from_untyped_with(
            UntypedDataSpecification::parse("map f: Nat # Bool # Nat -> Bool;").unwrap(),
            NumberEncoding::default(),
            &mut sources,
        )
        .unwrap();
        let sort = &checked.data_specification().map_declarations[0].sort;

        let generated = standard_sort(&mut sources, sort, NumberEncoding::Binary);
        let equations: Vec<String> = generated
            .equation_declarations
            .iter()
            .flat_map(|eqn_spec| &eqn_spec.equations)
            .map(|eqn| eqn.to_string())
            .collect();
        assert!(
            equations
                .iter()
                .any(|eqn| eqn.contains("@func_update(f, x0, x1, x2, v)")),
            "expected @func_update applied with all three index arguments: {equations:#?}"
        );
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_standard_sort_substitutes_binder_sorts() {
        // The set template's `==` equation quantifies over the element sort
        // (`forall c:S.`); instantiation must substitute binder sorts like any
        // declaration sort, or the generated equation would reference the
        // undeclared `S`.
        let spec = UntypedDataSpecification::parse("map f: Set(Nat);").unwrap();
        let generated = standard_sort(
            &mut SourceMap::new(),
            &spec.map_declarations[0].sort,
            NumberEncoding::Binary,
        );

        let equations: Vec<String> = generated
            .equation_declarations
            .iter()
            .flat_map(|eqn_spec| &eqn_spec.equations)
            .map(|eqn| eqn.to_string())
            .collect();
        assert!(
            equations.iter().any(|eqn| eqn.contains("forall c: Nat")),
            "the quantifier's binder sort should be instantiated: {equations:#?}"
        );
    }

    /// Extracts the constructors of the structured sort in `sort <ident> = <struct>;`.
    fn struct_constructors(spec: &str) -> Vec<ConstructorDecl> {
        let spec = UntypedDataSpecification::parse(spec).unwrap();
        let expr = spec
            .sort_declarations
            .into_iter()
            .find_map(|decl| decl.expr)
            .expect("expected a sort alias with a structured sort");
        let SortExpressionKind::Struct { inner } = expr.node else {
            panic!("expected a structured sort");
        };
        inner
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn structured_sort_equations_generates_a_parseable_specification() {
        let constructors = struct_constructors("sort D = struct c1(pr1: Nat, pr2: Bool)?is_c1 | c2?is_c2 | c3(Nat);");

        // The generated specification should be well-formed and parseable, and
        // contain only equations; the declarations come from desugaring.
        let generated = structured_sort_equations(&mut SourceMap::new(), &constructors).unwrap();
        assert!(generated.sort_declarations.is_empty());
        assert!(generated.constructor_declarations.is_empty());
        assert!(generated.map_declarations.is_empty());

        let equations = generated
            .equation_declarations
            .iter()
            .flat_map(|eqn_spec| &eqn_spec.equations)
            .map(|eqn| format!("{} = {}", eqn.lhs, eqn.rhs))
            .collect::<Vec<_>>();

        // Recogniser and projection equations for the declared names.
        assert!(
            equations
                .iter()
                .any(|eqn| eqn.contains("is_c1") && eqn.contains("true"))
        );
        assert!(
            equations
                .iter()
                .any(|eqn| eqn.contains("is_c1") && eqn.contains("false"))
        );
        assert!(equations.iter().any(|eqn| eqn.contains("pr1")));

        // c3 has no recogniser, so no equation defines one for it.
        assert!(!equations.iter().any(|eqn| eqn.contains("is_c3")));
    }

    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn structured_sort_equations_supports_only_constant_constructors() {
        // A structured sort where no constructor has arguments generates no
        // variables, so the `eqn` block must be emitted without a `var` block.
        let constructors = struct_constructors("sort E = struct red | green | blue;");
        let generated = structured_sort_equations(&mut SourceMap::new(), &constructors).unwrap();

        assert!(!generated.equation_declarations.is_empty());
    }

    /// A system-defined declaration's span must render against its true
    /// origin — the bundled template file it came from — not the caller's
    /// own specification text, once it is registered into the same
    /// `SourceMap` the caller renders against.
    #[test]
    #[cfg_attr(miri, ignore)] // Test is too slow under miri
    fn test_basic_sort_declaration_renders_against_its_builtin_source() {
        let mut sources = SourceMap::new();
        let basics = super::basic_sort_data_specification(&mut sources, NumberEncoding::Binary);
        let bool_decl = basics
            .sort_declarations
            .iter()
            .find(|decl| decl.identifier == "Bool")
            .expect("Bool is always declared");

        let rendered = bool_decl.span.render(&sources);
        assert!(
            rendered.contains("bool.mcrl2"),
            "expected the Bool sort declaration to render against bool.mcrl2, got: {rendered}"
        );
        let id = sources.lookup(bool_decl.span.start);
        assert!(sources.is_virtual(id), "a builtin template's source must be virtual");
    }
}
