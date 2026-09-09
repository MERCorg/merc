//! Tests for [`ProcessSpecification::typing_info`]: the span-keyed hover/go-to-definition
//! information accumulated over a checked process specification's action arguments,
//! process-instantiation arguments, conditions, time bounds, and `dist` weights.

use merc_syntax::Span;
use merc_syntax::UntypedProcessSpecification;
use merc_typecheck::ProcessSpecification;
use merc_typecheck::ResolvedName;
use merc_typecheck::TypingInfo;

/// Type checks `text` as a process specification, returning its full [`TypingInfo`] (every
/// process-body expression, merged).
#[track_caller]
fn typing_for(text: &str) -> TypingInfo {
    let spec = UntypedProcessSpecification::parse(text).expect("the specification should parse");
    let mut spec = ProcessSpecification::from_untyped(spec).expect("the specification should type check");
    spec.typing_info()
}

/// Finds `needle`'s byte offset in `text` and returns the sort (as displayed text) of the most
/// specific typed node there.
#[track_caller]
fn hover(text: &str, needle: &str) -> String {
    let offset = text
        .find(needle)
        .unwrap_or_else(|| panic!("'{needle}' not found in '{text}'"));
    typing_for(text)
        .at_offset(offset)
        .unwrap_or_else(|| panic!("no typed node at offset {offset} in '{text}'"))
        .sort
        .as_ref()
        .unwrap_or_else(|| panic!("node at offset {offset} in '{text}' has no sort (e.g. an action/process name)"))
        .to_string()
}

/// As [`hover`], but returns the resolved name instead of the sort.
#[track_caller]
fn resolved_name_at(text: &str, needle: &str) -> ResolvedName {
    let offset = text
        .find(needle)
        .unwrap_or_else(|| panic!("'{needle}' not found in '{text}'"));
    typing_for(text)
        .at_offset(offset)
        .unwrap_or_else(|| panic!("no typed node at offset {offset} in '{text}'"))
        .name
        .clone()
        .unwrap_or_else(|| panic!("node at offset {offset} in '{text}' has no resolved name"))
}

/// An action argument yields a non-empty, resolvable `TypingInfo`.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_action_argument_hover_reports_declared_sort() {
    assert_eq!(hover("act a: Nat; proc P(n: Nat) = a(n); init P(1);", "n);"), "Nat");
}

/// The declaration span carried by a `Variable` resolution points at the process's own
/// parameter declaration, not the occurrence — the actual goto-definition target.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_action_argument_goto_def_declaration_points_at_process_parameter() {
    let text = "act a: Nat; proc P(n: Nat) = a(n); init P(1);";
    let name = resolved_name_at(text, "n);");
    let ResolvedName::Variable { name, declaration } = &name else {
        panic!("expected a Variable resolution, got {name:?}");
    };
    assert_eq!(name, "n");
    let declaration = declaration
        .clone()
        .expect("a process parameter has a real declaration span");
    assert_eq!(&text[declaration.start..declaration.end], "n");
}

/// A `sum`-bound variable's declaration span points at the `sum` binder itself, not the
/// process's parameter list.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_sum_bound_variable_goto_def_declaration_points_at_binder() {
    let text = "act a: Nat; proc P = sum x: Nat . a(x); init P;";
    let name = resolved_name_at(text, "x);");
    let ResolvedName::Variable { declaration, .. } = &name else {
        panic!("expected a Variable resolution, got {name:?}");
    };
    let declaration = declaration
        .clone()
        .expect("a sum-bound variable has a real declaration span");
    assert_eq!(&text[declaration.start..declaration.end], "x");
}

/// A `sum` binder's own declaration occurrence (`x` in `sum x: Nat . ...`, not a later use of it
/// in the body) is itself hoverable.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_sum_bound_variable_declaration_itself_is_hoverable() {
    let text = "act a: Nat; proc P = sum x: Nat . a(x); init P;";
    assert_eq!(hover(text, "x: Nat"), "Nat");
    let name = resolved_name_at(text, "x: Nat");
    let ResolvedName::Variable { name, declaration } = &name else {
        panic!("expected a Variable resolution, got {name:?}");
    };
    assert_eq!(name, "x");
    let declaration = declaration
        .clone()
        .expect("a sum-bound variable has a real declaration span");
    assert_eq!(&text[declaration.start..declaration.end], "x");
}

/// A process's own parameter declaration (`n` in `P(n: Nat)`, not a use of it in the body) is
/// itself hoverable.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_process_parameter_declaration_itself_is_hoverable() {
    let text = "act a: Nat; proc P(n: Nat) = a(n); init P(1);";
    assert_eq!(hover(text, "n: Nat"), "Nat");
}

/// A `glob` variable's own declaration (not a use of it) is itself hoverable.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_global_variable_declaration_itself_is_hoverable() {
    let text = "act a: Nat; glob x: Nat; proc P = a(x); init P;";
    assert_eq!(hover(text, "x: Nat"), "Nat");
}

/// A condition's guard is checked (and its typing recorded) too, not just action arguments.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_condition_guard_hover_reports_bool() {
    assert_eq!(hover("act a; proc P(b: Bool) = b -> a; init P(true);", "b ->"), "Bool");
}

/// In a `+`-joined chain of guarded `sum`-actions, every branch's own bound variable must
/// resolve, not just the last one.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_every_branch_of_a_choice_chain_contributes_typing() {
    let text = "act a: Nat; b: Nat; c: Nat; \
                proc P(n: Nat) = sum x: Nat. a(x) + sum y: Nat. b(y) + sum z: Nat. c(z); \
                init P(0);";
    assert_eq!(hover(text, "x)"), "Nat");
    assert_eq!(hover(text, "y)"), "Nat");
    assert_eq!(hover(text, "z)"), "Nat");

    assert!(matches!(&resolved_name_at(text, "x)"), ResolvedName::Variable { name, .. } if name == "x"));
    assert!(matches!(&resolved_name_at(text, "y)"), ResolvedName::Variable { name, .. } if name == "y"));
    assert!(matches!(&resolved_name_at(text, "z)"), ResolvedName::Variable { name, .. } if name == "z"));
}

/// As [`test_every_branch_of_a_choice_chain_contributes_typing`], for a chain of guarded
/// conditions (`is_x(state) -> ... + is_y(state) -> ... + ...`): every guard's own condition must
/// be checked and typed, not just the last.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_every_guard_of_a_condition_chain_contributes_typing() {
    let text = "act a, b, c; \
                map is_a, is_b, is_c: Nat -> Bool; \
                proc P(state: Nat) = is_a(state) -> a + is_b(state) -> b + is_c(state) -> c; \
                init P(0);";
    // Looked up directly by span rather than through `hover`/`at_offset`: `is_x(state)` packs its
    // identifier, `(`, argument and `)` with no gap, so every offset inside it also sits on the
    // *end* of a narrower sub-node (`is_x` or `state`) — `at_offset`'s inclusive-end tie-break
    // always prefers that narrower node, so no offset resolves to the guard's own application.
    let info = typing_for(text);
    for guard in ["is_a(state)", "is_b(state)", "is_c(state)"] {
        let start = text.find(guard).expect("guard text should occur verbatim");
        let span = Span {
            start,
            end: start + guard.len(),
        };
        let node = info
            .nodes()
            .iter()
            .find(|node| node.span == span)
            .unwrap_or_else(|| panic!("no typed node spanning '{guard}'"));
        assert_eq!(
            node.sort
                .as_ref()
                .expect("the guard is a Bool-checked application")
                .to_string(),
            "Bool"
        );
    }
}

/// `a(n)`'s own name — not its argument — resolves to an `Action`, pointing at its `act`
/// declaration.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_action_name_goto_def_resolves_to_its_declaration() {
    let text = "act a: Nat; proc P(n: Nat) = a(n); init P(1);";
    let name = resolved_name_at(text, "a(n)");
    let ResolvedName::Action { name, declaration } = &name else {
        panic!("expected an Action resolution, got {name:?}");
    };
    assert_eq!(name, "a");
    let declaration = declaration.clone().expect("a plain `act` declaration has a real span");
    assert_eq!(&text[declaration.start..declaration.end], "a");
    assert_eq!(declaration.start, text.find("a: Nat").expect("'a: Nat' not found"));
}

/// A positional process instantiation (`P(1)`, parsed as the same `ProcessExprKind::Action` node
/// an action instance is) resolves to a `Process`, not an `Action`.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_positional_process_instantiation_name_goto_def_resolves_to_its_declaration() {
    let text = "proc P(n: Nat) = delta; init P(1);";
    let name = resolved_name_at(text, "P(1)");
    let ResolvedName::Process { name, declaration } = &name else {
        panic!("expected a Process resolution, got {name:?}");
    };
    assert_eq!(name, "P");
    let declaration = declaration.clone().expect("a plain `proc` declaration has a real span");
    assert_eq!(&text[declaration.start..declaration.end], "P");
    assert_eq!(
        declaration.start,
        text.find("P(n: Nat)").expect("'P(n: Nat)' not found")
    );
}

/// The assignment form of instantiation (`P(n = 1)`, a `ProcessExprKind::Id` node) resolves to a
/// `Process` the same way the positional form does.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_assignment_form_instantiation_name_goto_def_resolves_to_its_declaration() {
    let text = "proc P(n: Nat) = delta; init P(n = 1);";
    let name = resolved_name_at(text, "P(n = 1)");
    let ResolvedName::Process { name, declaration } = &name else {
        panic!("expected a Process resolution, got {name:?}");
    };
    assert_eq!(name, "P");
    let declaration = declaration.clone().expect("a plain `proc` declaration has a real span");
    assert_eq!(&text[declaration.start..declaration.end], "P");
}

/// An action overloaded by argument sort resolves each *use* to the specific declaration its
/// arguments actually match.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_overloaded_action_resolves_to_the_matching_declaration_by_arity() {
    let text = "act a: Nat; a: Bool; proc P = a(1) + a(true); init P;";
    let nat_decl = resolved_name_at(text, "a(1)");
    let ResolvedName::Action { declaration, .. } = &nat_decl else {
        panic!("expected an Action resolution, got {nat_decl:?}");
    };
    let declaration = declaration.clone().expect("a real declaration span");
    assert_eq!(declaration.start, text.find("a: Nat").expect("'a: Nat' not found"));

    let bool_decl = resolved_name_at(text, "a(true)");
    let ResolvedName::Action { declaration, .. } = &bool_decl else {
        panic!("expected an Action resolution, got {bool_decl:?}");
    };
    let declaration = declaration.clone().expect("a real declaration span");
    assert_eq!(declaration.start, text.find("a: Bool").expect("'a: Bool' not found"));
}

#[track_caller]
fn action_set_at(name: ResolvedName) -> (String, Vec<Span>) {
    let ResolvedName::ActionSet { name, declarations } = name else {
        panic!("expected an ActionSet resolution, got {name:?}");
    };
    (name, declarations)
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_hide_action_name_resolves_to_action_set() {
    let text = "act a, b; init hide({b}, a);";
    let (name, declarations) = action_set_at(resolved_name_at(text, "b}"));
    assert_eq!(name, "b");
    let [declaration] = declarations.as_slice() else {
        panic!("expected exactly one declaration, got {declarations:?}");
    };
    assert_eq!(&text[declaration.start..declaration.end], "b");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_block_action_name_resolves_to_action_set() {
    let text = "act a; init block({a}, a);";
    let (name, declarations) = action_set_at(resolved_name_at(text, "a}"));
    assert_eq!(name, "a");
    assert_eq!(declarations.len(), 1);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_allow_action_name_resolves_to_action_set() {
    let text = "act a, b; init allow({b}, a);";
    let (name, declarations) = action_set_at(resolved_name_at(text, "b}"));
    assert_eq!(name, "b");
    assert_eq!(declarations.len(), 1);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_comm_from_and_to_action_names_resolve_to_action_sets() {
    // `comm`'s multi-action `from` side needs at least two actions (`Id "|" MultActId`, see the
    // grammar), unlike `rename`'s plain `Id -> Id`.
    let text = "act a, c, b; proc P = b; init comm({a|c -> b}, P);";
    let (from_name, from_declarations) = action_set_at(resolved_name_at(text, "a|c"));
    assert_eq!(from_name, "a");
    assert_eq!(from_declarations.len(), 1);

    let (to_name, to_declarations) = action_set_at(resolved_name_at(text, "b}"));
    assert_eq!(to_name, "b");
    assert_eq!(to_declarations.len(), 1);
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_rename_from_and_to_action_names_resolve_to_action_sets() {
    let text = "act a, b; proc P = b; init rename({a -> b}, P);";
    let (from_name, from_declarations) = action_set_at(resolved_name_at(text, "a ->"));
    assert_eq!(from_name, "a");
    assert_eq!(from_declarations.len(), 1);

    let (to_name, to_declarations) = action_set_at(resolved_name_at(text, "b}"));
    assert_eq!(to_name, "b");
    assert_eq!(to_declarations.len(), 1);
}

/// An overloaded action named in an action-name set (no argument list to disambiguate by) offers
/// every declaration, in declaration order, rather than guessing one.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_overloaded_action_in_action_set_offers_every_declaration() {
    let text = "act b: Nat; b: Bool; a; init block({b}, a);";
    let (name, declarations) = action_set_at(resolved_name_at(text, "b}"));
    assert_eq!(name, "b");
    let [first, second] = declarations.as_slice() else {
        panic!("expected exactly two declarations, got {declarations:?}");
    };
    assert_eq!(first.start, text.find("b: Nat").expect("'b: Nat' not found"));
    assert_eq!(second.start, text.find("b: Bool").expect("'b: Bool' not found"));
}

/// An `act` declaration's own argument sort resolves to the `sort` block declaring it.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_action_argument_sort_goto_def_resolves_to_its_declaration() {
    let text = "sort D; cons c: D; act a: D; init a(c);";
    // The needle must start exactly on the `act`'s own "D" — `resolved_name_at` looks up its
    // needle's *start* offset, and both `sort D;` and `cons c: D;` also contain a "D;"; "D; init"
    // is unique to the `act` declaration's own sort.
    let name = resolved_name_at(text, "D; init");
    let ResolvedName::Sort { name, declaration } = &name else {
        panic!("expected a Sort resolution, got {name:?}");
    };
    assert_eq!(name, "D");
    let declaration = declaration.clone().expect("a plain `sort` declaration has a real span");
    assert_eq!(&text[declaration.start..declaration.end], "D");
}

/// A `proc` declaration's own parameter sort resolves the same way.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_process_parameter_sort_goto_def_resolves_to_its_declaration() {
    let text = "sort D; cons c: D; proc P(x: D) = delta; init P(c);";
    // Must start on "D", not "x" — "D)" is unique to the parameter's own sort.
    let name = resolved_name_at(text, "D)");
    let ResolvedName::Sort { name, declaration } = &name else {
        panic!("expected a Sort resolution, got {name:?}");
    };
    assert_eq!(name, "D");
    let declaration = declaration.clone().expect("a plain `sort` declaration has a real span");
    assert_eq!(&text[declaration.start..declaration.end], "D");
}

/// A `glob` variable's own sort resolves the same way.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_global_variable_sort_goto_def_resolves_to_its_declaration() {
    let text = "sort D; glob x: D; init delta;";
    // Must start on "D", not "x" — "D; init" is unique to the `glob`'s own sort (unlike
    // `sort D;`'s own "D;", which is followed by " glob", not " init").
    let name = resolved_name_at(text, "D; init");
    let ResolvedName::Sort { name, .. } = &name else {
        panic!("expected a Sort resolution, got {name:?}");
    };
    assert_eq!(name, "D");
}

/// A `sum` binder's own declared sort (not the bound variable itself, already covered by
/// [`test_sum_bound_variable_goto_def_declaration_points_at_binder`]) resolves too.
#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_sum_bound_variable_sort_goto_def_resolves_to_its_declaration() {
    let text = "sort D; cons c: D; act a: D; proc P = sum x: D . a(x); init P;";
    // "D . a(x)" is unique in `text` — the only "D" immediately followed by the binder's own
    // operand, as opposed to the `sort`/`cons`/`act` declarations' own "D" occurrences.
    let name = resolved_name_at(text, "D . a(x)");
    let ResolvedName::Sort { name, declaration } = &name else {
        panic!("expected a Sort resolution, got {name:?}");
    };
    assert_eq!(name, "D");
    let declaration = declaration.clone().expect("a plain `sort` declaration has a real span");
    assert_eq!(&text[declaration.start..declaration.end], "D");
}
