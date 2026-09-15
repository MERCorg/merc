use std::convert::Infallible;
use std::fmt;
use std::fmt::Write;
use std::sync::Arc;
use std::sync::LazyLock;

use merc_syntax::ComplexSort;
use merc_syntax::ConstructorDecl;
use merc_syntax::OffsetSpans;
use merc_syntax::SortExpression;
use merc_syntax::SortExpressionKind;
use merc_syntax::SourceMap;
use merc_syntax::Traverse;
use merc_syntax::TypeVarId;
use merc_syntax::UntypedDataSpecification;
use merc_utilities::MercError;

use crate::BUILTIN_SCHEME_TEMPLATE;
use crate::BUILTIN_SCHEME_TEMPLATE_TEXT;
use crate::InferenceError;
use crate::NumberEncoding;
use crate::Signature;
use crate::TypeCheckContext;
use crate::apply_sorts_in_spec;
use crate::assign_declaration_ids;
use crate::build_polymorphic_schemes;
use crate::check_template_equations;
use crate::lower_data_expressions;
use crate::merge_signatures;
use crate::resolve_data_specification_variables;
use crate::resolve_type_variables;

/// Identifies one Appendix-B template — bundled or generated — the same way
/// [`TypeCheckContext::template_typings`](crate::TypeCheckContext) keys it and
/// a [`TemplateInstantiation`](crate::TemplateInstantiation) names its source,
/// instead of the `String` each of those used to be keyed by (`"list"`,
/// `format!("function_update_{arity}")`, …).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum TemplateId {
    List,
    Set,
    FSet,
    Bag,
    FBag,
    /// The single-argument function-update template (`function_update.mcrl2`).
    FunctionUpdate,
    /// The generated, generic function-update template of the given arity
    /// (`> 1`; see [multi_argument_function_update_template]).
    FunctionUpdateN(usize),
    /// The reflexive/derived comparison-operator template
    /// (`crate::BUILTIN_SCHEME_TEMPLATE`).
    Comparison,
}

impl fmt::Display for TemplateId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TemplateId::List => write!(f, "list"),
            TemplateId::Set => write!(f, "set"),
            TemplateId::FSet => write!(f, "fset"),
            TemplateId::Bag => write!(f, "bag"),
            TemplateId::FBag => write!(f, "fbag"),
            TemplateId::FunctionUpdate => write!(f, "function_update"),
            TemplateId::FunctionUpdateN(arity) => write!(f, "function_update_{arity}"),
            TemplateId::Comparison => write!(f, "comparison"),
        }
    }
}

/// Parses a bundled `spec/*.mcrl2` file, or an equally self-contained
/// hand-written template string (`BUILTIN_SCHEME_TEMPLATE`), with no
/// `SourceMap` involved: the result's spans are meaningless outside `text`
/// itself. Used for [CONTAINER_TEMPLATES] and `BUILTIN_SCHEME_TEMPLATE`,
/// the two sources `build_polymorphic_schemes` draws from — nothing built
/// this way is ever rendered.
pub(crate) fn parse_template_bare(text: &str) -> UntypedDataSpecification {
    let mut spec = UntypedDataSpecification::parse(text).expect("the bundled templates parse");
    resolve_type_variables(&mut spec).expect("the bundled template's type_var block resolves");
    spec
}

/// As [parse_template_bare], but also assigns `VarId`s to the template's own
/// `var`-block variables and `EqnSpecId`/`EquationId`s to its equations.
pub(crate) fn parse_rigid_template(text: &str) -> UntypedDataSpecification {
    let mut spec = parse_template_bare(text);
    resolve_data_specification_variables(&mut spec);
    assign_declaration_ids(&mut spec);
    // Inference requires lowered expressions, exactly like `spec`/`system`;
    // idempotent, so `standard_sort`'s later `lower_data_expressions(&mut
    // generated)` on an instantiated clone of this template is a no-op.
    lower_data_expressions(&mut spec);
    spec
}

/// Registers `text` under `name` as a virtual source in `sources` (see
/// [SourceMap::add_virtual]) and parses it, then shifts every span it produced
/// into that registration's base offset — the same offsetting technique
/// [merc_syntax::imports] uses for `%import`, for content this module
/// generated itself (`write!` output rather than a bundled `spec/*.mcrl2`
/// file) and so, unlike a bundled template, might not parse — a bug in the
/// generator rather than in a `spec/*.mcrl2` file. Returns the parse error
/// instead of panicking, so a caller can report it.
fn parse_generated(sources: &mut SourceMap, name: &str, text: &str) -> Result<UntypedDataSpecification, MercError> {
    let id = sources.add_virtual(name, text.to_string());
    let base = sources.base_offset(id);
    let mut spec = UntypedDataSpecification::parse(text)?;
    spec.offset_spans(base);
    // As in `parse_template_bare`: resolves a template's own `type_var` block, if
    // it has one. Content this module generates itself (`multi_argument_function_update`,
    // `structured_sort_equations`) never declares one, so this is a no-op there.
    resolve_type_variables(&mut spec)?;
    Ok(spec)
}

/// Registers `text` under `name` as a virtual source in `sources`, the same as [parse_generated].
fn register_bare_template(
    sources: &mut SourceMap,
    name: &str,
    text: &'static str,
    template: &UntypedDataSpecification,
) -> UntypedDataSpecification {
    let id = sources.add_virtual(name, text);
    let base = sources.base_offset(id);
    let mut spec = template.clone();
    spec.offset_spans(base);
    spec
}

/// The raw, uninstantiated basic-sort templates, parsed once and span shifted
/// when necessary.
struct BasicSortTemplates {
    bool: UntypedDataSpecification,
    pos: UntypedDataSpecification,
    int: UntypedDataSpecification,
    nat: UntypedDataSpecification,
    real: UntypedDataSpecification,
    machine_word: UntypedDataSpecification,
    pos64: UntypedDataSpecification,
    int64: UntypedDataSpecification,
    nat64: UntypedDataSpecification,
    real64: UntypedDataSpecification,
}

static BASIC_SORT_TEMPLATES: LazyLock<BasicSortTemplates> = LazyLock::new(|| BasicSortTemplates {
    bool: parse_template_bare(include_str!("../../../syntax/spec/bool.mcrl2")),
    pos: parse_template_bare(include_str!("../../../syntax/spec/pos.mcrl2")),
    int: parse_template_bare(include_str!("../../../syntax/spec/int.mcrl2")),
    nat: parse_template_bare(include_str!("../../../syntax/spec/nat.mcrl2")),
    real: parse_template_bare(include_str!("../../../syntax/spec/real.mcrl2")),
    machine_word: parse_template_bare(include_str!("../../../syntax/spec/machine_word.mcrl2")),
    pos64: parse_template_bare(include_str!("../../../syntax/spec/pos64.mcrl2")),
    int64: parse_template_bare(include_str!("../../../syntax/spec/int64.mcrl2")),
    nat64: parse_template_bare(include_str!("../../../syntax/spec/nat64.mcrl2")),
    real64: parse_template_bare(include_str!("../../../syntax/spec/real64.mcrl2")),
});

/// Registers and merges each `(virtual path, source text, parsed template)`
/// triple in `entries` into one specification, in order — the shared body of
/// [basic_sorts_binary]/[basic_sorts_machine_word].
fn merge_bare_templates(
    sources: &mut SourceMap,
    entries: &[(&'static str, &'static str, &UntypedDataSpecification)],
) -> UntypedDataSpecification {
    let mut result = UntypedDataSpecification::default();
    for &(name, text, template) in entries {
        result.merge(&register_bare_template(sources, name, text, template));
    }
    result
}

/// The merged specifications of the five basic sorts (Appendix B) in the
/// recursive binary encoding.
fn basic_sorts_binary(sources: &mut SourceMap) -> UntypedDataSpecification {
    merge_bare_templates(
        sources,
        &[
            (
                "<builtin>/bool.mcrl2",
                include_str!("../../../syntax/spec/bool.mcrl2"),
                &BASIC_SORT_TEMPLATES.bool,
            ),
            (
                "<builtin>/pos.mcrl2",
                include_str!("../../../syntax/spec/pos.mcrl2"),
                &BASIC_SORT_TEMPLATES.pos,
            ),
            (
                "<builtin>/int.mcrl2",
                include_str!("../../../syntax/spec/int.mcrl2"),
                &BASIC_SORT_TEMPLATES.int,
            ),
            (
                "<builtin>/nat.mcrl2",
                include_str!("../../../syntax/spec/nat.mcrl2"),
                &BASIC_SORT_TEMPLATES.nat,
            ),
            (
                "<builtin>/real.mcrl2",
                include_str!("../../../syntax/spec/real.mcrl2"),
                &BASIC_SORT_TEMPLATES.real,
            ),
        ],
    )
}

/// The same five basic sorts in the 64-bit machine-word encoding. `Bool` is
/// shared with the binary encoding; the numeric sorts come from the `*64`
/// templates, which are defined in terms of the `@word` sort that
/// `machine_word.mcrl2` declares.
fn basic_sorts_machine_word(sources: &mut SourceMap) -> UntypedDataSpecification {
    merge_bare_templates(
        sources,
        &[
            (
                "<builtin>/bool.mcrl2",
                include_str!("../../../syntax/spec/bool.mcrl2"),
                &BASIC_SORT_TEMPLATES.bool,
            ),
            (
                "<builtin>/machine_word.mcrl2",
                include_str!("../../../syntax/spec/machine_word.mcrl2"),
                &BASIC_SORT_TEMPLATES.machine_word,
            ),
            (
                "<builtin>/pos64.mcrl2",
                include_str!("../../../syntax/spec/pos64.mcrl2"),
                &BASIC_SORT_TEMPLATES.pos64,
            ),
            (
                "<builtin>/int64.mcrl2",
                include_str!("../../../syntax/spec/int64.mcrl2"),
                &BASIC_SORT_TEMPLATES.int64,
            ),
            (
                "<builtin>/nat64.mcrl2",
                include_str!("../../../syntax/spec/nat64.mcrl2"),
                &BASIC_SORT_TEMPLATES.nat64,
            ),
            (
                "<builtin>/real64.mcrl2",
                include_str!("../../../syntax/spec/real64.mcrl2"),
                &BASIC_SORT_TEMPLATES.real64,
            ),
        ],
    )
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

/// The [TemplateId] of each [ContainerTemplates] field, in the same order as
/// [ContainerTemplates::all]/[ContainerTemplates::all_named].
const CONTAINER_TEMPLATE_IDS: [TemplateId; 6] = [
    TemplateId::List,
    TemplateId::Set,
    TemplateId::FSet,
    TemplateId::Bag,
    TemplateId::FBag,
    TemplateId::FunctionUpdate,
];

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

    /// As [Self::all], paired with each template's own [TemplateId] from
    /// [CONTAINER_TEMPLATE_IDS].
    pub(crate) fn all_named(&self) -> [(TemplateId, &UntypedDataSpecification); 6] {
        let templates = self.all();
        std::array::from_fn(|i| (CONTAINER_TEMPLATE_IDS[i], templates[i]))
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
    list: parse_rigid_template(include_str!("../../../syntax/spec/list.mcrl2")),
    set: parse_rigid_template(include_str!("../../../syntax/spec/set.mcrl2")),
    fset: parse_rigid_template(include_str!("../../../syntax/spec/fset.mcrl2")),
    bag: parse_rigid_template(include_str!("../../../syntax/spec/bag.mcrl2")),
    fbag: parse_rigid_template(include_str!("../../../syntax/spec/fbag.mcrl2")),
    function_update: parse_rigid_template(include_str!("../../../syntax/spec/function_update.mcrl2")),
});

/// As [CONTAINER_TEMPLATES], for the container templates whose equations are expressed in terms of
/// the machine-word numeric sorts. `function_update` is left unparsed here — it mentions no
/// numbers, so [container_templates_machine_word] shares [CONTAINER_TEMPLATES]'s copy instead.
static CONTAINER_TEMPLATES_MACHINE_WORD: LazyLock<ContainerTemplates> = LazyLock::new(|| ContainerTemplates {
    list: parse_rigid_template(include_str!("../../../syntax/spec/list64.mcrl2")),
    set: parse_rigid_template(include_str!("../../../syntax/spec/set64.mcrl2")),
    fset: parse_rigid_template(include_str!("../../../syntax/spec/fset64.mcrl2")),
    bag: parse_rigid_template(include_str!("../../../syntax/spec/bag64.mcrl2")),
    fbag: parse_rigid_template(include_str!("../../../syntax/spec/fbag64.mcrl2")),
    function_update: parse_rigid_template(include_str!("../../../syntax/spec/function_update.mcrl2")),
});

/// The container templates in the recursive binary encoding, registered into
/// `sources` as virtual documents — the content-producing counterpart of
/// [CONTAINER_TEMPLATES], used wherever the result joins a [DataSpecification]'s
/// `system` and so needs spans that render correctly.
///
/// [DataSpecification]: crate::DataSpecification
fn container_templates_binary(sources: &mut SourceMap) -> ContainerTemplates {
    ContainerTemplates {
        list: register_bare_template(
            sources,
            "<builtin>/list.mcrl2",
            include_str!("../../../syntax/spec/list.mcrl2"),
            &CONTAINER_TEMPLATES.list,
        ),
        set: register_bare_template(
            sources,
            "<builtin>/set.mcrl2",
            include_str!("../../../syntax/spec/set.mcrl2"),
            &CONTAINER_TEMPLATES.set,
        ),
        fset: register_bare_template(
            sources,
            "<builtin>/fset.mcrl2",
            include_str!("../../../syntax/spec/fset.mcrl2"),
            &CONTAINER_TEMPLATES.fset,
        ),
        bag: register_bare_template(
            sources,
            "<builtin>/bag.mcrl2",
            include_str!("../../../syntax/spec/bag.mcrl2"),
            &CONTAINER_TEMPLATES.bag,
        ),
        fbag: register_bare_template(
            sources,
            "<builtin>/fbag.mcrl2",
            include_str!("../../../syntax/spec/fbag.mcrl2"),
            &CONTAINER_TEMPLATES.fbag,
        ),
        function_update: register_bare_template(
            sources,
            "<builtin>/function_update.mcrl2",
            include_str!("../../../syntax/spec/function_update.mcrl2"),
            &CONTAINER_TEMPLATES.function_update,
        ),
    }
}

/// The container templates whose equations are expressed in terms of the
/// machine-word numeric sorts. `function_update.mcrl2` mentions no numbers, so
/// it is shared with the binary encoding.
fn container_templates_machine_word(sources: &mut SourceMap) -> ContainerTemplates {
    ContainerTemplates {
        list: register_bare_template(
            sources,
            "<builtin>/list64.mcrl2",
            include_str!("../../../syntax/spec/list64.mcrl2"),
            &CONTAINER_TEMPLATES_MACHINE_WORD.list,
        ),
        set: register_bare_template(
            sources,
            "<builtin>/set64.mcrl2",
            include_str!("../../../syntax/spec/set64.mcrl2"),
            &CONTAINER_TEMPLATES_MACHINE_WORD.set,
        ),
        fset: register_bare_template(
            sources,
            "<builtin>/fset64.mcrl2",
            include_str!("../../../syntax/spec/fset64.mcrl2"),
            &CONTAINER_TEMPLATES_MACHINE_WORD.fset,
        ),
        bag: register_bare_template(
            sources,
            "<builtin>/bag64.mcrl2",
            include_str!("../../../syntax/spec/bag64.mcrl2"),
            &CONTAINER_TEMPLATES_MACHINE_WORD.bag,
        ),
        fbag: register_bare_template(
            sources,
            "<builtin>/fbag64.mcrl2",
            include_str!("../../../syntax/spec/fbag64.mcrl2"),
            &CONTAINER_TEMPLATES_MACHINE_WORD.fbag,
        ),
        function_update: register_bare_template(
            sources,
            "<builtin>/function_update.mcrl2",
            include_str!("../../../syntax/spec/function_update.mcrl2"),
            &CONTAINER_TEMPLATES.function_update,
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

/// Type checks every container/function-update template's own equations
/// once, with its type variable(s) held rigid, populating
/// `ctx.template_typings` (see `check_template_equations`) — whichever
/// template set `encoding` will actually be instantiated from below.
/// Idempotent: a template already present in `ctx.template_typings` is
/// skipped.
pub(crate) fn check_container_templates(
    ctx: &mut TypeCheckContext,
    encoding: NumberEncoding,
) -> Result<(), InferenceError> {
    let templates: &ContainerTemplates = match encoding {
        NumberEncoding::Binary => &CONTAINER_TEMPLATES,
        NumberEncoding::MachineWord => &CONTAINER_TEMPLATES_MACHINE_WORD,
    };

    for (id, template) in templates.all_named() {
        if !ctx.template_typings.contains_key(&id) {
            let typings = check_template_equations(ctx, template)?;
            ctx.template_typings.insert(id, typings);
        }
    }
    Ok(())
}

/// The multi-argument counterpart of [check_container_templates]: type
/// checks the generic, arity-`arity` function-update template's own
/// equations once, with its type variable(s) held rigid, populating
/// `ctx.template_typings` under [`TemplateId::FunctionUpdateN`] (the same id
/// [standard_sort_with_provenance] records for an instantiation of this
/// arity). Idempotent.
pub(crate) fn check_multi_argument_function_update_template(
    ctx: &mut TypeCheckContext,
    arity: usize,
) -> Result<(), InferenceError> {
    let id = TemplateId::FunctionUpdateN(arity);
    if ctx.template_typings.contains_key(&id) {
        return Ok(());
    }
    let template = multi_argument_function_update_template(arity);

    // This arity's own `@func_update`/`@func_update_stable`/`@is_not_an_update`/
    // `@if_always_else` are declared only inside `template` itself — the
    // pooled `ctx.signature` only carries the bundled, single-argument
    // `function_update.mcrl2`'s versions of those same names, which would
    // fail to unify against an arity-`arity` application.
    let original_signature = Arc::clone(
        ctx.signature
            .as_ref()
            .expect("build_signature ran before check_multi_argument_function_update_template"),
    );
    let own_schemes = build_polymorphic_schemes(ctx, std::iter::once(&template));
    let own_signature = Signature {
        schemes: own_schemes,
        ..Signature::default()
    };
    ctx.signature = Some(Arc::new(merge_signatures(&own_signature, &original_signature)));

    let typings = check_template_equations(ctx, &template);
    ctx.signature = Some(original_signature);

    ctx.template_typings.insert(id, typings?);
    Ok(())
}

/// Type checks `crate::BUILTIN_SCHEME_TEMPLATE`'s own `var`/`eqn` block once.
pub(crate) fn check_comparison_template(ctx: &mut TypeCheckContext) -> Result<(), InferenceError> {
    if ctx.template_typings.contains_key(&TemplateId::Comparison) {
        return Ok(());
    }

    let typings = check_template_equations(ctx, &BUILTIN_SCHEME_TEMPLATE)?;
    ctx.template_typings.insert(TemplateId::Comparison, typings);
    Ok(())
}

/// As [standard_sort_with_provenance], but instantiates the reflexive/derived
/// comparison-operator equations (`crate::BUILTIN_SCHEME_TEMPLATE`'s own
/// `eqn` block) for `sort` instead of a container/function-update template —
/// applies uniformly to any concrete sort, since the template holds only one
/// `type_var S` and no branching on `sort`'s shape.
///
/// Registers a fresh virtual document per call, the same way
/// [container_templates_binary]/[container_templates_machine_word] do for a
/// bundled container template: `BUILTIN_SCHEME_TEMPLATE` itself is parsed
/// once with no `SourceMap` involved (see [parse_template_bare]), so without
/// this its spans would render against nothing.
///
/// Only equations are returned; the `map` signatures are dropped after
/// substitution, deliberately. Unlike a container operation (`in`, `count`,
/// …), `==`/`<`/`if` are looked up purely as the pooled scheme at lowering
/// time too — `mcrl2_lowering`'s builtin-name arm builds the concrete
/// `DataFunctionSymbol` directly from a use site's already-resolved sort, with
/// no matching `map` declaration required anywhere in the generated system
/// content — so a monomorphic `map ==: List(Nat) # List(Nat) -> Bool;` per
/// instantiated sort would be pure, unbounded bloat on `system` for no
/// consumer.
pub(crate) fn comparison_operator_equations_with_provenance(
    sources: &mut SourceMap,
    sort: &SortExpression,
) -> (UntypedDataSpecification, (TemplateId, Vec<SortExpression>)) {
    let template = register_bare_template(
        sources,
        "<builtin>/schemes/comparison.mcrl2",
        BUILTIN_SCHEME_TEMPLATE_TEXT,
        &BUILTIN_SCHEME_TEMPLATE,
    );
    let mut generated = replace_sort(&template, "S", sort);
    generated.map_declarations.clear();
    (generated, (TemplateId::Comparison, vec![sort.clone()]))
}

/// Returns a standard data specification containing the standard sorts and their
/// associated constructors, mappings, and equations, in the given `encoding`,
/// registered into `sources`.
pub(crate) fn basic_sort_data_specification(
    sources: &mut SourceMap,
    encoding: NumberEncoding,
) -> UntypedDataSpecification {
    match encoding {
        NumberEncoding::Binary => basic_sorts_binary(sources),
        NumberEncoding::MachineWord => basic_sorts_machine_word(sources),
    }
}

/// Constructs a data specification for a standard sort, in the given
/// `encoding`, registered into `sources`.
pub(crate) fn standard_sort(
    sources: &mut SourceMap,
    sort: &SortExpression,
    encoding: NumberEncoding,
) -> UntypedDataSpecification {
    standard_sort_with_provenance(sources, sort, encoding).0
}

/// As [standard_sort], but also returns which [TemplateId] (bundled or
/// generic) produced the result, and the concrete sort(s) substituted for its
/// `type_var` declaration(s), in declaration order. Used by
/// [`crate::merge_generated`] to record a [`crate::TemplateInstantiation`]
/// for later specialization instead of re-checking each generated equation
/// from scratch.
pub(crate) fn standard_sort_with_provenance(
    sources: &mut SourceMap,
    sort: &SortExpression,
    encoding: NumberEncoding,
) -> (UntypedDataSpecification, (TemplateId, Vec<SortExpression>)) {
    let templates = container_templates(sources, encoding);

    if let SortExpressionKind::Complex(complex, element) = &sort.node {
        let (id, template) = match complex {
            ComplexSort::List => (TemplateId::List, &templates.list),
            ComplexSort::Set => (TemplateId::Set, &templates.set),
            ComplexSort::FSet => (TemplateId::FSet, &templates.fset),
            ComplexSort::Bag => (TemplateId::Bag, &templates.bag),
            ComplexSort::FBag => (TemplateId::FBag, &templates.fbag),
        };

        (replace_sort(template, "S", element), (id, vec![(**element).clone()]))
    } else if let SortExpressionKind::Function { domain, range } = &sort.node {
        // In the specification we define the function S -> T.
        let spec = replace_sort(&templates.function_update, "S", domain);
        (
            replace_sort(&spec, "T", range),
            (TemplateId::FunctionUpdate, vec![(**domain).clone(), (**range).clone()]),
        )
    } else if let SortExpressionKind::FlattenedFunction { domain, range } = &sort.node {
        // A multi-argument function sort: the bundled template's single index
        // variable `S` cannot stand for a product, so its own generic,
        // arity-parameterized template is built (and, once per arity, checked)
        // separately — see `multi_argument_function_update`.
        let arity = domain.len();
        let mut substitution = domain.clone();
        substitution.push((**range).clone());
        (
            multi_argument_function_update(sources, domain, range),
            (TemplateId::FunctionUpdateN(arity), substitution),
        )
    } else {
        unreachable!("The given sort {} is not a standard sort", sort);
    }
}

/// The generic function-update template of arity `arity > 1`: `type_var S0,
/// ..., S{arity-1}, T;` in place of concrete domain/range sorts, generated,
/// parsed and prepared for equation checking exactly like
/// [parse_rigid_template] — regenerated (cheaply — it's a handful of
/// equations) each time it's needed rather than cached: its `TypeVarId`s are
/// deterministic (always `0..=arity` in declaration order for a given
/// arity), so any two independently parsed copies agree, and
/// `ctx.template_typings`'s own [`TemplateId::FunctionUpdateN`] entry
/// (built once by `check_multi_argument_function_update_template`) is the
/// only thing that actually needs to persist. Mirrors the bundled
/// single-argument `function_update.mcrl2` template, just generated rather
/// than bundled since its arity isn't known ahead of time.
fn multi_argument_function_update_template(arity: usize) -> UntypedDataSpecification {
    debug_assert!(arity > 1, "single-argument function updates use the bundled template");

    let domain_names: Vec<String> = (0..arity).map(|i| format!("S{i}")).collect();
    let range_name = "T";
    let text = format!(
        "type_var {}, {range_name};\n{}",
        domain_names.join(", "),
        multi_argument_function_update_text(&domain_names, range_name)
    );

    let mut spec = UntypedDataSpecification::parse(&text).unwrap_or_else(|err| {
        panic!("the generated arity-{arity} function-update template does not parse: {err}\n{text}")
    });
    resolve_type_variables(&mut spec).expect("the generated template's type_var block resolves");
    resolve_data_specification_variables(&mut spec);
    assign_declaration_ids(&mut spec);
    lower_data_expressions(&mut spec);
    spec
}

/// Generates the function-update operators (`@func_update`,
/// `@func_update_stable`, `@is_not_an_update`, `@if_always_else`, Appendix
/// B.11 / `function_update.mcrl2`) for a function sort of arity
/// `domain.len() > 1`, generalizing the bundled single-argument template to
/// the flattened domain `D_0 # ... # D_{n-1} -> T` — by substituting the
/// concrete domain/range sorts into [multi_argument_function_update_template]'s
/// generic, arity-matched template, exactly like [standard_sort]'s own
/// substitution of a concrete element sort into a bundled container template.
pub(crate) fn multi_argument_function_update(
    sources: &mut SourceMap,
    domain: &[SortExpression],
    range: &SortExpression,
) -> UntypedDataSpecification {
    let arity = domain.len();
    debug_assert!(
        arity > 1,
        "single-argument function updates are generated from the bundled template"
    );

    let template = multi_argument_function_update_template(arity);
    let mut spec = template;
    for (i, argument_sort) in domain.iter().enumerate() {
        spec = replace_sort(&spec, &format!("S{i}"), argument_sort);
    }
    let mut spec = replace_sort(&spec, "T", range);

    // Registers a rendering of this concrete instantiation, purely for error
    // display: `spec`'s own spans still point at the *template*'s content,
    // which — since only the domain/range sort text was substituted — reads
    // identically to this rendering, so offsetting them into it is exact.
    let domain_sorts = domain
        .iter()
        .map(SortExpression::to_string)
        .collect::<Vec<_>>()
        .join(" # ");
    let domain_names: Vec<String> = domain.iter().map(SortExpression::to_string).collect();
    let text = multi_argument_function_update_text(&domain_names, &range.to_string());
    let id = sources.add_virtual(
        format!("<generated>/function_update({domain_sorts} -> {range}).mcrl2"),
        text,
    );
    let base = sources.base_offset(id);
    spec.offset_spans(base);
    spec
}

/// The body (`map`/`var`/`eqn` blocks) of the function-update operators for
/// arity `domain_names.len() > 1`, over the given domain/range sort *text* —
/// either symbolic `type_var` names (building the generic template, see
/// [multi_argument_function_update_template]) or concrete sort text (unused
/// today, since instantiation now substitutes into the template instead, but
/// kept general).
///
/// The single index variable `x`/`y` of the unary template becomes a tuple
/// `x0, ..., x{n-1}`: two tuples are compared componentwise for equality
/// (`&&` of `==`) and ordered lexicographically (`<`) wherever the template
/// orders or compares a single index — `<` canonicalizes the order nested
/// `@func_update_stable` chains normalize to, so rewriting stays confluent
/// regardless of the syntactic nesting order of `f[a -> b][c -> d]`-style
/// updates. This mirrors [structured_sort_equations]'s `lexicographic` helper,
/// which solves the same problem for a constructor's argument tuple.
fn multi_argument_function_update_text(domain_names: &[String], range: &str) -> String {
    let arity = domain_names.len();
    debug_assert!(arity > 1, "single-argument function updates use the bundled template");

    // Parenthesized: this text is embedded as one operand alongside others in
    // `@func_update`'s own domain/range below, and a bare `A # B -> C` would
    // parse with the wrong grouping there. The original `SortExpression`-based
    // version of this function got this for free from `Display`'s own
    // parenthesization of a nested function sort; built from plain text now,
    // it has to be added explicitly.
    let function_sort = format!("({} -> {range})", domain_names.join(" # "));
    let domain_sorts = domain_names.join(" # ");

    let xs: Vec<String> = (0..arity).map(|i| format!("x{i}")).collect();
    let ys: Vec<String> = (0..arity).map(|i| format!("y{i}")).collect();
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
    for (i, argument_sort) in domain_names.iter().enumerate() {
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

    spec
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

/// Replaces every [SortExpressionKind::ResolvedTypeVar] node naming `type_var_id` in `sort` by
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
        if let Some(recogniser) = &constructor.recogniser {
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
