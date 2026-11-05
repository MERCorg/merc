//! Lowering a whole type checked data specification, including its system-defined equations.

use merc_data::Mcrl2DataSpecification;
use merc_lowering::lower_data_specification;
use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_mcrl2_data_specification_sections_populated() {
    let spec = DataSpecification::from_untyped(
        UntypedDataSpecification::parse(
            "sort D; \
             sort A = Nat; \
             cons c: D; \
             map f: D -> Bool; \
             var d: D; \
             eqn f(d) = true;",
        )
        .unwrap(),
    )
    .unwrap();
    let mcrl2 = lower_data_specification(&spec);

    // User abstract sort `D` → sorts section.
    assert!(mcrl2.sorts().iter().any(|s| s.name() == "D"), "D must appear in sorts");
    // User alias `A = Nat` → aliases section.
    assert!(
        mcrl2.aliases().iter().any(|a| a.name().name() == "A"),
        "A must appear in aliases"
    );
    // User constructor `c` → constructors section.
    assert!(
        mcrl2.constructors().iter().any(|c| c.name() == "c"),
        "c must appear in constructors"
    );
    // User mapping `f` → mappings section.
    assert!(
        mcrl2.mappings().iter().any(|m| m.name() == "f"),
        "f must appear in mappings"
    );
    // User equation `f(d) = true` → equations section.
    assert!(
        !mcrl2.equations().is_empty(),
        "at least one user equation must be lowered"
    );
    assert_eq!(mcrl2.equations()[0].lhs().to_string(), "f(d)");
    assert_eq!(mcrl2.equations()[0].rhs().to_string(), "true");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_mcrl2_data_specification_system_constructors_present() {
    // `Bool` always pulls in its system constructors; at least `true`/`false` must appear.
    let spec = DataSpecification::from_untyped(UntypedDataSpecification::parse("map f: Bool;").unwrap()).unwrap();
    let mcrl2 = lower_data_specification(&spec);
    assert!(
        mcrl2.constructors().iter().any(|c| c.name() == "true"),
        "system Bool constructor `true` must appear in constructors"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_mcrl2_data_specification_system_equations_present() {
    // System Bool equations (e.g. `!true = false`) must appear now that
    // `lower_data_specification` includes structurally-lowerable system equations.
    let spec = DataSpecification::from_untyped(UntypedDataSpecification::parse("map f: Bool;").unwrap()).unwrap();
    let mcrl2 = lower_data_specification(&spec);
    // `!true = false` should be among the system Bool equations.
    let found = mcrl2
        .equations()
        .iter()
        .any(|e| e.lhs().to_string().contains("!(true)") && e.rhs().to_string() == "false");
    assert!(found, "system Bool equation `!(true) = false` must be present");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_mcrl2_data_specification_empty_container_equations_present() {
    // A `List(D)` sort pulls in the Appendix-B list equations, several of
    // which mention the empty-list literal `[]` (`in(d, []) = false`,
    // `#[] = @c0`). These are now lowered structurally via expected-sort
    // propagation rather than skipped.
    let spec =
        DataSpecification::from_untyped(UntypedDataSpecification::parse("sort D; map f: List(D) -> Bool;").unwrap())
            .unwrap();
    let mcrl2 = lower_data_specification(&spec);

    // `in(d, []) = false` — the empty list as a `List(D)` argument.
    let in_empty = mcrl2
        .equations()
        .iter()
        .any(|e| e.lhs().to_string() == "in(d, [])" && e.rhs().to_string() == "false");
    assert!(in_empty, "system list equation `in(d, []) = false` must be present");

    // `#[] = @c0` — the empty list under the length operator.
    let length_empty = mcrl2
        .equations()
        .iter()
        .any(|e| e.lhs().to_string() == "#([])" && e.rhs().to_string() == "@c0");
    assert!(length_empty, "system list equation `#[] = @c0` must be present");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_mcrl2_data_specification_enumeration_literal_container_equations_present() {
    // `List(Nat)` never occurs as a textual sort here — only as the
    // element sort of the list-enumeration literal `[1, 2, 3]`, which
    // `collect_system_sorts_in_spec`'s syntactic scan cannot see (its own
    // doc comment notes enumeration literals are not syntactically
    // apparent). Its Appendix-B equations must still be instantiated from
    // the inferred sort during lowering.
    let spec =
        DataSpecification::from_untyped(UntypedDataSpecification::parse("map f: Bool; eqn f = 1 in [2, 3];").unwrap())
            .unwrap();
    let mcrl2 = lower_data_specification(&spec);

    let in_empty = mcrl2
        .equations()
        .iter()
        .any(|e| e.lhs().to_string() == "in(d, [])" && e.rhs().to_string() == "false");
    assert!(
        in_empty,
        "system list equation `in(d, []) = false` for List(Nat) must be present: {:#?}",
        mcrl2.equations().iter().map(|e| e.to_string()).collect::<Vec<_>>()
    );

    // The `|>` (cons) constructor for `List(Nat)` must also be declared,
    // not just its equations.
    assert!(
        mcrl2.constructors().iter().any(|c| c.name() == "|>"),
        "the List(Nat) cons constructor must be present"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_mcrl2_data_specification_set_enumeration_literal_container_equations_present() {
    // As above, but for a set-enumeration literal (`FSet(Nat)`, never
    // declared textually).
    let spec =
        DataSpecification::from_untyped(UntypedDataSpecification::parse("map n: Nat; eqn n = #{1, 2, 3};").unwrap())
            .unwrap();
    let mcrl2 = lower_data_specification(&spec);

    let in_empty = mcrl2
        .equations()
        .iter()
        .any(|e| e.lhs().to_string() == "in(d, {})" && e.rhs().to_string() == "false");
    assert!(
        in_empty,
        "system fset equation `in(d, {{}}) = false` for FSet(Nat) must be present: {:#?}",
        mcrl2.equations().iter().map(|e| e.to_string()).collect::<Vec<_>>()
    );
    assert!(
        mcrl2.constructors().iter().any(|c| c.name() == "@fset_insert"),
        "the FSet(Nat) @fset_insert constructor must be present"
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_mcrl2_data_specification_bag_enumeration_literal_container_equations_present() {
    // As above, but for a bag-enumeration literal (`FBag(Nat)`, never
    // declared textually).
    let spec =
        DataSpecification::from_untyped(UntypedDataSpecification::parse("map n: Nat; eqn n = #{1: 2, 3: 4};").unwrap())
            .unwrap();
    let mcrl2 = lower_data_specification(&spec);

    assert!(
        mcrl2.mappings().iter().any(|m| m.name() == "@fbag_cinsert"),
        "the FBag(Nat) @fbag_cinsert mapping must be present: {:#?}",
        mcrl2
            .mappings()
            .iter()
            .map(|m| m.name().to_string())
            .collect::<Vec<_>>()
    );
}

/// Formats every lowered equation as `lhs = rhs`, for assertion messages.
fn equation_strings(mcrl2: &Mcrl2DataSpecification) -> Vec<String> {
    mcrl2
        .equations()
        .iter()
        .map(|e| format!("{} = {}", e.lhs(), e.rhs()))
        .collect()
}

// The tests below guard that a system equation using a binder, a bare
// higher-order name value, or a struct-desugared symbol reaches the lowered
// output rather than being dropped.

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_set_extensionality_equation_survives_lowering() {
    // `set.mcrl2`'s `@set(f, s) == @set(g, t) = forall c:S. ...`.
    let spec =
        DataSpecification::from_untyped(UntypedDataSpecification::parse("map f: Set(Nat) -> Bool;").unwrap()).unwrap();
    let mcrl2 = lower_data_specification(&spec);
    let found = mcrl2
        .equations()
        .iter()
        .any(|e| e.lhs().to_string().contains("==") && e.rhs().to_string().contains("Forall"));
    assert!(
        found,
        "the Set extensionality equation (a forall in its rhs) must survive lowering: {:#?}",
        equation_strings(&mcrl2)
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_bag_extensionality_equation_survives_lowering() {
    // `bag.mcrl2`'s counterpart of the Set extensionality equation.
    let spec =
        DataSpecification::from_untyped(UntypedDataSpecification::parse("map f: Bag(Nat) -> Bool;").unwrap()).unwrap();
    let mcrl2 = lower_data_specification(&spec);
    let found = mcrl2
        .equations()
        .iter()
        .any(|e| e.lhs().to_string().contains("==") && e.rhs().to_string().contains("Forall"));
    assert!(
        found,
        "the Bag extensionality equation (a forall in its rhs) must survive lowering: {:#?}",
        equation_strings(&mcrl2)
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_bare_higher_order_value_equation_survives_lowering() {
    // `set.mcrl2`'s `@setfset(s) = @set(@false_, s)`, where `@false_` is used
    // point-free (`S -> Bool`, never applied).
    let spec =
        DataSpecification::from_untyped(UntypedDataSpecification::parse("map f: Set(Nat) -> Bool;").unwrap()).unwrap();
    let mcrl2 = lower_data_specification(&spec);
    let found = mcrl2
        .equations()
        .iter()
        .any(|e| e.lhs().to_string() == "@setfset(s)" && e.rhs().to_string() == "@set(@false_, s)");
    assert!(
        found,
        "the '@setfset(s) = @set(@false_, s)' equation must survive lowering: {:#?}",
        equation_strings(&mcrl2)
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_struct_recogniser_and_projection_equations_survive_lowering() {
    // A struct's recogniser/projection equations reference symbols (`is_c1`,
    // `pr1`, `c1`) declared on the user spec by struct desugaring, not on
    // the system spec.
    let spec = DataSpecification::from_untyped(
        UntypedDataSpecification::parse("sort D = struct c1(pr1: Nat, pr2: Bool)?is_c1 | c2?is_c2; map f: D -> Bool;")
            .unwrap(),
    )
    .unwrap();
    let mcrl2 = lower_data_specification(&spec);

    let recogniser = mcrl2
        .equations()
        .iter()
        .any(|e| e.lhs().to_string() == "is_c1(c1(@x0_0, @x0_1))" && e.rhs().to_string() == "true");
    assert!(
        recogniser,
        "the recogniser equation 'is_c1(c1(@x0_0, @x0_1)) = true' must survive lowering: {:#?}",
        equation_strings(&mcrl2)
    );

    let projection = mcrl2
        .equations()
        .iter()
        .any(|e| e.lhs().to_string() == "pr1(c1(@x0_0, @x0_1))" && e.rhs().to_string() == "@x0_0");
    assert!(
        projection,
        "the projection equation 'pr1(c1(@x0_0, @x0_1)) = @x0_0' must survive lowering: {:#?}",
        equation_strings(&mcrl2)
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_multiple_element_sorts_of_the_same_container_do_not_collide() {
    // `Bag(Nat)` and `Bag(D)` each carry their own copy of `bag.mcrl2`'s
    // `@zero_ == @one_ = false;`, which pins down no instantiation and would
    // be ambiguous against one pooled signature — see `SystemEquationGroup`.
    let spec = DataSpecification::from_untyped(
        UntypedDataSpecification::parse("sort D = struct d1; map f: Bag(Nat) -> Bool; g: Bag(D) -> Bool;").unwrap(),
    )
    .unwrap();
    let mcrl2 = lower_data_specification(&spec);

    assert_eq!(
        mcrl2.mappings().iter().filter(|m| m.name() == "@zero_").count(),
        2,
        "both Bag(Nat) and Bag(D) should declare their own @zero_: {:#?}",
        mcrl2
            .mappings()
            .iter()
            .map(|m| m.name().to_string())
            .collect::<Vec<_>>()
    );

    let zero_eq_one_false_count = mcrl2
        .equations()
        .iter()
        .filter(|e| e.lhs().to_string() == "==(@zero_, @one_)" && e.rhs().to_string() == "false")
        .count();
    assert_eq!(
        zero_eq_one_false_count,
        2,
        "'@zero_ == @one_ = false' should survive once per instantiation: {:#?}",
        equation_strings(&mcrl2)
    );
}
