use merc_syntax::UntypedDataSpecification;
use merc_typecheck::DataSpecification;
use merc_typecheck::WellTypedError;

/// Type checks `text`, asserting it is accepted (nano-crl2 `should_compile`).
#[track_caller]
fn check_ok(text: &str) {
    let spec = UntypedDataSpecification::parse(text).expect("the specification should parse");
    if let Err(err) = DataSpecification::from_untyped(spec) {
        panic!("expected the specification to type check, got {err}:\n{text}");
    }
}

/// Type checks `text`, asserting it is rejected (nano-crl2 `should_not_compile`).
#[track_caller]
fn check_err(text: &str) -> WellTypedError {
    let spec = UntypedDataSpecification::parse(text).expect("the specification should parse");
    match DataSpecification::from_untyped(spec) {
        Err(err) => err,
        Ok(_) => panic!("expected the specification to be rejected:\n{text}"),
    }
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_circular_constructors() {
    // Two independent sort groups. Each has a well-founded "escape": `A` via
    // `y2: Nat -> A`, and `C`/`D` via the alias chain that bottoms out in
    // `Nat`. Circular *references* between constructors are fine as long as
    // every sort has one syntactically non-empty constructor.
    check_ok(
        "sort A, B;
         cons x: A -> B;
         cons y1: B -> A;
         cons y2: Nat -> A;

         sort C, D;
         cons v: C -> D;
         cons w1: Alias2;
         cons w2: Alias1;
         sort Alias1 = Nat -> C;
         sort Alias2 = D -> C;",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_infinite_recursion() {
    // An equation whose right-hand side recurses forever is still well typed;
    // termination is not a type-checking concern.
    check_ok(
        "map f: Nat -> Nat;
         var i: Nat;
         eqn f(i) = f(1 + 1);",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_ops() {
    check_ok(
        "map f: Int -> Int;
             g: Nat;
         var x: Int, y: Int;
             z: Nat;
         eqn f(x + (y + z)) = min(y * x - z, 1);
             x != y -> x == y = false;
             g = max(x, z);",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_sets() {
    check_ok(
        "map n1: Nat;
         eqn n1 = 42;

         map f1: Set(Bool);
         eqn f1 = { b: Bool | b || !b };

         map f2: FSet(Nat);
         eqn f2 = { 0, 1, 7, 7 };

         map f3: FSet(Nat);
         eqn f3 = { };
         eqn f3 = {};

         map f4: FBag(Nat);
         eqn f4 = { n1: 7, 4: 7 };

         map f5: FBag(Nat);
         eqn f5 = { n1: 7 };

         map f6: FBag(Nat);
         eqn f6 = {:};",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_structs_data() {
    // The data-specification portion of `should_compile/structs`. `Rec` is
    // well-founded through `foo3(List(Rec))` (the empty list is a base case),
    // and anonymous `struct` sorts are legal in a `map` domain/range.
    check_ok(
        "sort Rec = struct foo(Rec) | foo2(b: Rec, c: Rec) | foo3(l: List(Rec)) ? is_list;

         map a: struct cons1 | cons2 -> Nat;",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_well_typed1() {
    // Example 15.1.13
    check_ok(
        "map f: Real # Nat -> Bool;
         eqn f(0, 1) = false;",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_well_typed2() {
    // Example 15.1.15
    check_ok(
        "map f: Real # Nat -> Nat;
             f: Nat # Real -> Real;
         eqn f(0, 0) = f(0, 0);
             f(0, 0) = 0;",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_well_typed3() {
    // Example 15.1.16
    check_ok(
        "map f: Real -> Nat -> Bool;
             f: Nat -> Real -> Bool;
         eqn f(0)(0) = false;",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_well_typed_arg_order() {
    // Example 15.1.14
    check_ok(
        "map f: Real # Nat -> Bool;
             f: Nat # Real -> Bool;
         eqn f(0, 0) = false;",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_well_typed_function_update() {
    check_ok(
        "map g: (Nat -> Nat) -> Nat -> Nat;
         var f: Nat -> Nat;
         eqn g(f) = f[f(0) -> 5];",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_well_typed_if() {
    // `y` and `z` have a common sort `Set(Int)` (`FSet(Int) <= Set(Int)`).
    check_ok(
        "var x: Pos;
             y: Set(Int);
             z: FSet(Int);
         eqn if(false, x, 0) = 0;
             if(true, y, z) = y;",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_well_typed_literals() {
    check_ok(
        "map f: Pos -> Nat;
             f: Nat -> Nat;
         map g: Nat -> Nat;
             g: Int -> Nat;
         map h: Int -> Nat;
             h: Real -> Nat;
         map i: FSet(Nat) -> Nat;
             i: Set(Nat) -> Nat;
         map j: FBag(Nat) -> Nat;
             j: Bag(Nat) -> Nat;
         map k: FSet(Nat) -> Nat;
             k: Set(Nat) -> Bool;

         eqn f(1) = 0;
             g(0) = 0;
             h(0) = 0;
             i({ 1, 2 }) = 0;
             j({ 1: 2 }) = 0;
             k({ 1 }) = true;",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_well_typed_rr_not_mono() {
    check_ok(
        "map f: Nat -> Set(Int);
             f: Nat -> FSet(Nat);
         map g: Nat -> Set(Int);
             g: Nat -> Set(Pos);
         var x: Nat;
         eqn f(x) = g(x);",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_well_typed_whr() {
    // Contrast to `test_not_well_typed_whr`.
    check_ok(
        "map f: Bool -> Bool;
             f: Real -> Bool;
             h: (Bool -> Bool) -> Bool;
         eqn f(0) && h(f) -> 0 = 0;",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_well_typed_whr_num() {
    check_ok(
        "map x: Nat -> Nat;
         map y: Int;
         eqn y = plus(z, z) whr z = 1 end;

         sort T = Pos;

         map plus: Nat # Nat -> Nat;
             plus: Pos # Pos -> Pos;
             plus: Pos # Nat -> Pos;
             plus: Nat # Pos -> Pos;
             plus: Int # Int -> Int;
             plus: Real # Real -> Real;
             plus: FSet(T) # FSet(T) -> FSet(T);
             plus: Set(T) # Set(T) -> Set(T);
             plus: FBag(T) # FBag(T) -> FBag(T);
             plus: Bag(T) # Bag(T) -> Bag(T);

         eqn plus({}, {}) = {};
             plus(0, 0) = 0;",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_circular_alias() {
    let err = check_err(
        "sort A = B;
         sort B = C;
         sort C = D;
         sort D = A;",
    );
    assert!(matches!(err, WellTypedError::AliasCycle { .. }), "got {err:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_circular_constructors1() {
    // `A` and `B` are mutually recursive with no base case: neither is
    // syntactically non-empty, so both are rejected as empty sorts.
    let err = check_err(
        "sort A, B;
         cons x: A -> B;
         cons y: B -> A;",
    );
    assert!(matches!(err, WellTypedError::EmptySort { .. }), "got {err:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_circular_constructors2() {
    // Same as above but the function sorts are hidden behind aliases.
    let err = check_err(
        "sort A, B;
         cons x: Alias1;
         cons y: Alias2;
         sort Alias1 = A -> B;
         sort Alias2 = B -> A;",
    );
    assert!(matches!(err, WellTypedError::EmptySort { .. }), "got {err:?}");
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_not_well_typed_function_update() {
    // The update key `0` has sort `Nat`, but `f: Pos -> Nat` requires a `Pos`
    // key, and `Nat` does not downcast to `Pos`.
    check_err(
        "map g: (Pos -> Nat) -> Pos -> Nat;
         var f: Pos -> Nat;
         eqn g(f) = f[0 -> f(3)];",
    );
}

#[test]
#[cfg_attr(miri, ignore)] // Test is too slow under miri
fn test_not_well_typed_whr() {
    // Example 15.1.16, second expression. `x` is bound to the ambiguous `f`
    // while simultaneously being applied as `x(0)` and passed to
    // `h: (Bool -> Bool) -> Bool`, which cannot be reconciled.
    check_err(
        "map f: Bool -> Bool;
             f: Real -> Bool;
             h: (Bool -> Bool) -> Bool;
         eqn x(0) && h(x) whr x = f end -> 0 = 0;",
    );
}
